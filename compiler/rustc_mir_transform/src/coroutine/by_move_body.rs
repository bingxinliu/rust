//! This pass constructs a second coroutine body sufficient for return from
//! `FnOnce`/`AsyncFnOnce` implementations for coroutine-closures (e.g. async closures).
//!
//! Consider an async closure like:
//! ```rust
//! #![feature(async_closure)]
//!
//! let x = vec![1, 2, 3];
//!
//! let closure = async move || {
//!     println!("{x:#?}");
//! };
//! ```
//!
//! This desugars to something like:
//! ```rust,ignore (invalid-borrowck)
//! let x = vec![1, 2, 3];
//!
//! let closure = move || {
//!     async {
//!         println!("{x:#?}");
//!     }
//! };
//! ```
//!
//! Important to note here is that while the outer closure *moves* `x: Vec<i32>`
//! into its upvars, the inner `async` coroutine simply captures a ref of `x`.
//! This is the "magic" of async closures -- the futures that they return are
//! allowed to borrow from their parent closure's upvars.
//!
//! However, what happens when we call `closure` with `AsyncFnOnce` (or `FnOnce`,
//! since all async closures implement that too)? Well, recall the signature:
//! ```
//! use std::future::Future;
//! pub trait AsyncFnOnce<Args>
//! {
//!     type CallOnceFuture: Future<Output = Self::Output>;
//!     type Output;
//!     fn async_call_once(
//!         self,
//!         args: Args
//!     ) -> Self::CallOnceFuture;
//! }
//! ```
//!
//! This signature *consumes* the async closure (`self`) and returns a `CallOnceFuture`.
//! How do we deal with the fact that the coroutine is supposed to take a reference
//! to the captured `x` from the parent closure, when that parent closure has been
//! destroyed?
//!
//! This is the second piece of magic of async closures. We can simply create a
//! *second* `async` coroutine body where that `x` that was previously captured
//! by reference is now captured by value. This means that we consume the outer
//! closure and return a new coroutine that will hold onto all of these captures,
//! and drop them when it is finished (i.e. after it has been `.await`ed).
//!
//! We do this with the analysis below, which detects the captures that come from
//! borrowing from the outer closure, and we simply peel off a `deref` projection
//! from them. This second body is stored alongside the first body, and optimized
//! with it in lockstep. When we need to resolve a body for `FnOnce` or `AsyncFnOnce`,
//! we use this "by-move" body instead.
//!
//! ## How does this work?
//!
//! This pass essentially remaps the body of the (child) closure of the coroutine-closure
//! to take the set of upvars of the parent closure by value. This at least requires
//! changing a by-ref upvar to be by-value in the case that the outer coroutine-closure
//! captures something by value; however, it may also require renumbering field indices
//! in case precise captures (edition 2021 closure capture rules) caused the inner coroutine
//! to split one field capture into two.

use rustc_data_structures::unord::UnordMap;
use rustc_hir as hir;
use rustc_middle::hir::place::{PlaceBase, Projection, ProjectionKind};
use rustc_middle::mir::visit::MutVisitor;
use rustc_middle::mir::{self, dump_mir, MirPass};
use rustc_middle::ty::{self, InstanceDef, Ty, TyCtxt, TypeVisitableExt};
use rustc_target::abi::{FieldIdx, VariantIdx};

pub struct ByMoveBody;

impl<'tcx> MirPass<'tcx> for ByMoveBody {
    fn run_pass(&self, tcx: TyCtxt<'tcx>, body: &mut mir::Body<'tcx>) {
        // We only need to generate by-move coroutine bodies for coroutines that come
        // from coroutine-closures.
        let Some(coroutine_def_id) = body.source.def_id().as_local() else {
            return;
        };
        let Some(hir::CoroutineKind::Desugared(_, hir::CoroutineSource::Closure)) =
            tcx.coroutine_kind(coroutine_def_id)
        else {
            return;
        };

        // Also, let's skip processing any bodies with errors, since there's no guarantee
        // the MIR body will be constructed well.
        let coroutine_ty = body.local_decls[ty::CAPTURE_STRUCT_LOCAL].ty;
        if coroutine_ty.references_error() {
            return;
        }

        // We don't need to generate a by-move coroutine if the coroutine body was
        // produced by the `CoroutineKindShim`, since it's already by-move.
        if matches!(body.source.instance, ty::InstanceDef::CoroutineKindShim { .. }) {
            return;
        }

        let ty::Coroutine(_, args) = *coroutine_ty.kind() else { bug!("{body:#?}") };
        let args = args.as_coroutine();

        let coroutine_kind = args.kind_ty().to_opt_closure_kind().unwrap();

        let parent_def_id = tcx.local_parent(coroutine_def_id);
        let ty::CoroutineClosure(_, parent_args) =
            *tcx.type_of(parent_def_id).instantiate_identity().kind()
        else {
            bug!();
        };
        let parent_closure_args = parent_args.as_coroutine_closure();
        let num_args = parent_closure_args
            .coroutine_closure_sig()
            .skip_binder()
            .tupled_inputs_ty
            .tuple_fields()
            .len();

        let mut field_remapping = UnordMap::default();

        // One parent capture may correspond to several child captures if we end up
        // refining the set of captures via edition-2021 precise captures. We want to
        // match up any number of child captures with one parent capture, so we keep
        // peeking off this `Peekable` until the child doesn't match anymore.
        let mut parent_captures =
            tcx.closure_captures(parent_def_id).iter().copied().enumerate().peekable();
        // Make sure we use every field at least once, b/c why are we capturing something
        // if it's not used in the inner coroutine.
        let mut field_used_at_least_once = false;

        for (child_field_idx, child_capture) in tcx
            .closure_captures(coroutine_def_id)
            .iter()
            .copied()
            // By construction we capture all the args first.
            .skip(num_args)
            .enumerate()
        {
            loop {
                let Some(&(parent_field_idx, parent_capture)) = parent_captures.peek() else {
                    bug!("we ran out of parent captures!")
                };
                // A parent matches a child they share the same prefix of projections.
                // The child may have more, if it is capturing sub-fields out of
                // something that is captured by-move in the parent closure.
                if !child_prefix_matches_parent_projections(parent_capture, child_capture) {
                    // Make sure the field was used at least once.
                    assert!(
                        field_used_at_least_once,
                        "we captured {parent_capture:#?} but it was not used in the child coroutine?"
                    );
                    field_used_at_least_once = false;
                    // Skip this field.
                    let _ = parent_captures.next().unwrap();
                    continue;
                }

                // Store this set of additional projections (fields and derefs).
                // We need to re-apply them later.
                let child_precise_captures =
                    &child_capture.place.projections[parent_capture.place.projections.len()..];

                // If the parent captures by-move, and the child captures by-ref, then we
                // need to peel an additional `deref` off of the body of the child.
                let needs_deref = child_capture.is_by_ref() && !parent_capture.is_by_ref();
                if needs_deref {
                    assert_ne!(
                        coroutine_kind,
                        ty::ClosureKind::FnOnce,
                        "`FnOnce` coroutine-closures return coroutines that capture from \
                        their body; it will always result in a borrowck error!"
                    );
                }

                // Finally, store the type of the parent's captured place. We need
                // this when building the field projection in the MIR body later on.
                let mut parent_capture_ty = parent_capture.place.ty();
                parent_capture_ty = match parent_capture.info.capture_kind {
                    ty::UpvarCapture::ByValue => parent_capture_ty,
                    ty::UpvarCapture::ByRef(kind) => Ty::new_ref(
                        tcx,
                        tcx.lifetimes.re_erased,
                        parent_capture_ty,
                        kind.to_mutbl_lossy(),
                    ),
                };

                field_remapping.insert(
                    FieldIdx::from_usize(child_field_idx + num_args),
                    (
                        FieldIdx::from_usize(parent_field_idx + num_args),
                        parent_capture_ty,
                        needs_deref,
                        child_precise_captures,
                    ),
                );

                field_used_at_least_once = true;
                break;
            }
        }

        // Pop the last parent capture
        if field_used_at_least_once {
            let _ = parent_captures.next().unwrap();
        }
        assert_eq!(parent_captures.next(), None, "leftover parent captures?");

        if coroutine_kind == ty::ClosureKind::FnOnce {
            assert_eq!(field_remapping.len(), tcx.closure_captures(parent_def_id).len());
            return;
        }

        let by_move_coroutine_ty = tcx
            .instantiate_bound_regions_with_erased(parent_closure_args.coroutine_closure_sig())
            .to_coroutine_given_kind_and_upvars(
                tcx,
                parent_closure_args.parent_args(),
                coroutine_def_id.to_def_id(),
                ty::ClosureKind::FnOnce,
                tcx.lifetimes.re_erased,
                parent_closure_args.tupled_upvars_ty(),
                parent_closure_args.coroutine_captures_by_ref_ty(),
            );

        let mut by_move_body = body.clone();
        MakeByMoveBody { tcx, field_remapping, by_move_coroutine_ty }.visit_body(&mut by_move_body);
        dump_mir(tcx, false, "coroutine_by_move", &0, &by_move_body, |_, _| Ok(()));
        by_move_body.source = mir::MirSource::from_instance(InstanceDef::CoroutineKindShim {
            coroutine_def_id: coroutine_def_id.to_def_id(),
        });
        body.coroutine.as_mut().unwrap().by_move_body = Some(by_move_body);
    }
}

fn child_prefix_matches_parent_projections(
    parent_capture: &ty::CapturedPlace<'_>,
    child_capture: &ty::CapturedPlace<'_>,
) -> bool {
    let PlaceBase::Upvar(parent_base) = parent_capture.place.base else {
        bug!("expected capture to be an upvar");
    };
    let PlaceBase::Upvar(child_base) = child_capture.place.base else {
        bug!("expected capture to be an upvar");
    };

    assert!(child_capture.place.projections.len() >= parent_capture.place.projections.len());
    parent_base.var_path.hir_id == child_base.var_path.hir_id
        && std::iter::zip(&child_capture.place.projections, &parent_capture.place.projections)
            .all(|(child, parent)| child.kind == parent.kind)
}

struct MakeByMoveBody<'tcx> {
    tcx: TyCtxt<'tcx>,
    field_remapping: UnordMap<FieldIdx, (FieldIdx, Ty<'tcx>, bool, &'tcx [Projection<'tcx>])>,
    by_move_coroutine_ty: Ty<'tcx>,
}

impl<'tcx> MutVisitor<'tcx> for MakeByMoveBody<'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn visit_place(
        &mut self,
        place: &mut mir::Place<'tcx>,
        context: mir::visit::PlaceContext,
        location: mir::Location,
    ) {
        // Initializing an upvar local always starts with `CAPTURE_STRUCT_LOCAL` and a
        // field projection. If this is in `field_remapping`, then it must not be an
        // arg from calling the closure, but instead an upvar.
        if place.local == ty::CAPTURE_STRUCT_LOCAL
            && let Some((&mir::ProjectionElem::Field(idx, _), projection)) =
                place.projection.split_first()
            && let Some(&(remapped_idx, remapped_ty, needs_deref, additional_projections)) =
                self.field_remapping.get(&idx)
        {
            // As noted before, if the parent closure captures a field by value, and
            // the child captures a field by ref, then for the by-move body we're
            // generating, we also are taking that field by value. Peel off a deref,
            // since a layer of reffing has now become redundant.
            let final_deref = if needs_deref {
                let Some((mir::ProjectionElem::Deref, projection)) = projection.split_first()
                else {
                    bug!(
                        "There should be at least a single deref for an upvar local initialization, found {projection:#?}"
                    );
                };
                // There may be more derefs, since we may also implicitly reborrow
                // a captured mut pointer.
                projection
            } else {
                projection
            };

            // The only thing that should be left is a deref, if the parent captured
            // an upvar by-ref.
            std::assert_matches::assert_matches!(final_deref, [] | [mir::ProjectionElem::Deref]);

            // For all of the additional projections that come out of precise capturing,
            // re-apply these projections.
            let additional_projections =
                additional_projections.iter().map(|elem| match elem.kind {
                    ProjectionKind::Deref => mir::ProjectionElem::Deref,
                    ProjectionKind::Field(idx, VariantIdx::ZERO) => {
                        mir::ProjectionElem::Field(idx, elem.ty)
                    }
                    _ => unreachable!("precise captures only through fields and derefs"),
                });

            // We start out with an adjusted field index (and ty), representing the
            // upvar that we get from our parent closure. We apply any of the additional
            // projections to make sure that to the rest of the body of the closure, the
            // place looks the same, and then apply that final deref if necessary.
            *place = mir::Place {
                local: place.local,
                projection: self.tcx.mk_place_elems_from_iter(
                    [mir::ProjectionElem::Field(remapped_idx, remapped_ty)]
                        .into_iter()
                        .chain(additional_projections)
                        .chain(final_deref.iter().copied()),
                ),
            };
        }
        self.super_place(place, context, location);
    }

    fn visit_local_decl(&mut self, local: mir::Local, local_decl: &mut mir::LocalDecl<'tcx>) {
        // Replace the type of the self arg.
        if local == ty::CAPTURE_STRUCT_LOCAL {
            local_decl.ty = self.by_move_coroutine_ty;
        }
    }
}
