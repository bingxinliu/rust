// Copyright 2012-2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use hir::def_id::DefId;
use ty::subst::Substs;
use ty::{ClosureSubsts, Region, Ty, GeneratorInterior};
use mir::*;
use rustc_const_math::ConstUsize;
use syntax_pos::Span;

// # The MIR Visitor
//
// ## Overview
//
// There are two visitors, one for immutable and one for mutable references,
// but both are generated by the following macro. The code is written according
// to the following conventions:
//
// - introduce a `visit_foo` and a `super_foo` method for every MIR type
// - `visit_foo`, by default, calls `super_foo`
// - `super_foo`, by default, destructures the `foo` and calls `visit_foo`
//
// This allows you as a user to override `visit_foo` for types are
// interested in, and invoke (within that method) call
// `self.super_foo` to get the default behavior. Just as in an OO
// language, you should never call `super` methods ordinarily except
// in that circumstance.
//
// For the most part, we do not destructure things external to the
// MIR, e.g. types, spans, etc, but simply visit them and stop. This
// avoids duplication with other visitors like `TypeFoldable`.
//
// ## Updating
//
// The code is written in a very deliberate style intended to minimize
// the chance of things being overlooked. You'll notice that we always
// use pattern matching to reference fields and we ensure that all
// matches are exhaustive.
//
// For example, the `super_basic_block_data` method begins like this:
//
// ```rust
// fn super_basic_block_data(&mut self,
//                           block: BasicBlock,
//                           data: & $($mutability)* BasicBlockData<'tcx>) {
//     let BasicBlockData {
//         ref $($mutability)* statements,
//         ref $($mutability)* terminator,
//         is_cleanup: _
//     } = *data;
//
//     for statement in statements {
//         self.visit_statement(block, statement);
//     }
//
//     ...
// }
// ```
//
// Here we used `let BasicBlockData { <fields> } = *data` deliberately,
// rather than writing `data.statements` in the body. This is because if one
// adds a new field to `BasicBlockData`, one will be forced to revise this code,
// and hence one will (hopefully) invoke the correct visit methods (if any).
//
// For this to work, ALL MATCHES MUST BE EXHAUSTIVE IN FIELDS AND VARIANTS.
// That means you never write `..` to skip over fields, nor do you write `_`
// to skip over variants in a `match`.
//
// The only place that `_` is acceptable is to match a field (or
// variant argument) that does not require visiting, as in
// `is_cleanup` above.

macro_rules! make_mir_visitor {
    ($visitor_trait_name:ident, $($mutability:ident)*) => {
        pub trait $visitor_trait_name<'tcx> {
            // Override these, and call `self.super_xxx` to revert back to the
            // default behavior.

            fn visit_mir(&mut self, mir: & $($mutability)* Mir<'tcx>) {
                self.super_mir(mir);
            }

            fn visit_basic_block_data(&mut self,
                                      block: BasicBlock,
                                      data: & $($mutability)* BasicBlockData<'tcx>) {
                self.super_basic_block_data(block, data);
            }

            fn visit_visibility_scope_data(&mut self,
                                           scope_data: & $($mutability)* VisibilityScopeData) {
                self.super_visibility_scope_data(scope_data);
            }

            fn visit_statement(&mut self,
                               block: BasicBlock,
                               statement: & $($mutability)* Statement<'tcx>,
                               location: Location) {
                self.super_statement(block, statement, location);
            }

            fn visit_assign(&mut self,
                            block: BasicBlock,
                            place: & $($mutability)* Place<'tcx>,
                            rvalue: & $($mutability)* Rvalue<'tcx>,
                            location: Location) {
                self.super_assign(block, place, rvalue, location);
            }

            fn visit_terminator(&mut self,
                                block: BasicBlock,
                                terminator: & $($mutability)* Terminator<'tcx>,
                                location: Location) {
                self.super_terminator(block, terminator, location);
            }

            fn visit_terminator_kind(&mut self,
                                     block: BasicBlock,
                                     kind: & $($mutability)* TerminatorKind<'tcx>,
                                     location: Location) {
                self.super_terminator_kind(block, kind, location);
            }

            fn visit_assert_message(&mut self,
                                    msg: & $($mutability)* AssertMessage<'tcx>,
                                    location: Location) {
                self.super_assert_message(msg, location);
            }

            fn visit_rvalue(&mut self,
                            rvalue: & $($mutability)* Rvalue<'tcx>,
                            location: Location) {
                self.super_rvalue(rvalue, location);
            }

            fn visit_operand(&mut self,
                             operand: & $($mutability)* Operand<'tcx>,
                             location: Location) {
                self.super_operand(operand, location);
            }

            fn visit_place(&mut self,
                            place: & $($mutability)* Place<'tcx>,
                            context: PlaceContext<'tcx>,
                            location: Location) {
                self.super_place(place, context, location);
            }

            fn visit_static(&mut self,
                            static_: & $($mutability)* Static<'tcx>,
                            context: PlaceContext<'tcx>,
                            location: Location) {
                self.super_static(static_, context, location);
            }

            fn visit_projection(&mut self,
                                place: & $($mutability)* PlaceProjection<'tcx>,
                                context: PlaceContext<'tcx>,
                                location: Location) {
                self.super_projection(place, context, location);
            }

            fn visit_projection_elem(&mut self,
                                     place: & $($mutability)* PlaceElem<'tcx>,
                                     context: PlaceContext<'tcx>,
                                     location: Location) {
                self.super_projection_elem(place, context, location);
            }

            fn visit_branch(&mut self,
                            source: BasicBlock,
                            target: BasicBlock) {
                self.super_branch(source, target);
            }

            fn visit_constant(&mut self,
                              constant: & $($mutability)* Constant<'tcx>,
                              location: Location) {
                self.super_constant(constant, location);
            }

            fn visit_literal(&mut self,
                             literal: & $($mutability)* Literal<'tcx>,
                             location: Location) {
                self.super_literal(literal, location);
            }

            fn visit_def_id(&mut self,
                            def_id: & $($mutability)* DefId,
                            _: Location) {
                self.super_def_id(def_id);
            }

            fn visit_span(&mut self,
                          span: & $($mutability)* Span) {
                self.super_span(span);
            }

            fn visit_source_info(&mut self,
                                 source_info: & $($mutability)* SourceInfo) {
                self.super_source_info(source_info);
            }

            fn visit_ty(&mut self,
                        ty: & $($mutability)* Ty<'tcx>,
                        _: TyContext) {
                self.super_ty(ty);
            }

            fn visit_region(&mut self,
                            region: & $($mutability)* ty::Region<'tcx>,
                            _: Location) {
                self.super_region(region);
            }

            fn visit_const(&mut self,
                           constant: & $($mutability)* &'tcx ty::Const<'tcx>,
                           _: Location) {
                self.super_const(constant);
            }

            fn visit_substs(&mut self,
                            substs: & $($mutability)* &'tcx Substs<'tcx>,
                            _: Location) {
                self.super_substs(substs);
            }

            fn visit_closure_substs(&mut self,
                                    substs: & $($mutability)* ClosureSubsts<'tcx>,
                                    _: Location) {
                self.super_closure_substs(substs);
            }

            fn visit_generator_interior(&mut self,
                                    interior: & $($mutability)* GeneratorInterior<'tcx>,
                                    _: Location) {
                self.super_generator_interior(interior);
            }

            fn visit_const_int(&mut self,
                               const_int: &ConstInt,
                               _: Location) {
                self.super_const_int(const_int);
            }

            fn visit_const_usize(&mut self,
                                 const_usize: & $($mutability)* ConstUsize,
                                 _: Location) {
                self.super_const_usize(const_usize);
            }

            fn visit_local_decl(&mut self,
                                local: Local,
                                local_decl: & $($mutability)* LocalDecl<'tcx>) {
                self.super_local_decl(local, local_decl);
            }

            fn visit_local(&mut self,
                            _local: & $($mutability)* Local,
                            _context: PlaceContext<'tcx>,
                            _location: Location) {
            }

            fn visit_visibility_scope(&mut self,
                                      scope: & $($mutability)* VisibilityScope) {
                self.super_visibility_scope(scope);
            }

            // The `super_xxx` methods comprise the default behavior and are
            // not meant to be overridden.

            fn super_mir(&mut self,
                         mir: & $($mutability)* Mir<'tcx>) {
                // for best performance, we want to use an iterator rather
                // than a for-loop, to avoid calling Mir::invalidate for
                // each basic block.
                macro_rules! basic_blocks {
                    (mut) => (mir.basic_blocks_mut().iter_enumerated_mut());
                    () => (mir.basic_blocks().iter_enumerated());
                };
                for (bb, data) in basic_blocks!($($mutability)*) {
                    self.visit_basic_block_data(bb, data);
                }

                for scope in &$($mutability)* mir.visibility_scopes {
                    self.visit_visibility_scope_data(scope);
                }

                self.visit_ty(&$($mutability)* mir.return_ty(), TyContext::ReturnTy(SourceInfo {
                    span: mir.span,
                    scope: ARGUMENT_VISIBILITY_SCOPE,
                }));

                for local in mir.local_decls.indices() {
                    self.visit_local_decl(local, & $($mutability)* mir.local_decls[local]);
                }

                self.visit_span(&$($mutability)* mir.span);
            }

            fn super_basic_block_data(&mut self,
                                      block: BasicBlock,
                                      data: & $($mutability)* BasicBlockData<'tcx>) {
                let BasicBlockData {
                    ref $($mutability)* statements,
                    ref $($mutability)* terminator,
                    is_cleanup: _
                } = *data;

                let mut index = 0;
                for statement in statements {
                    let location = Location { block: block, statement_index: index };
                    self.visit_statement(block, statement, location);
                    index += 1;
                }

                if let Some(ref $($mutability)* terminator) = *terminator {
                    let location = Location { block: block, statement_index: index };
                    self.visit_terminator(block, terminator, location);
                }
            }

            fn super_visibility_scope_data(&mut self,
                                           scope_data: & $($mutability)* VisibilityScopeData) {
                let VisibilityScopeData {
                    ref $($mutability)* span,
                    ref $($mutability)* parent_scope,
                } = *scope_data;

                self.visit_span(span);
                if let Some(ref $($mutability)* parent_scope) = *parent_scope {
                    self.visit_visibility_scope(parent_scope);
                }
            }

            fn super_statement(&mut self,
                               block: BasicBlock,
                               statement: & $($mutability)* Statement<'tcx>,
                               location: Location) {
                let Statement {
                    ref $($mutability)* source_info,
                    ref $($mutability)* kind,
                } = *statement;

                self.visit_source_info(source_info);
                match *kind {
                    StatementKind::Assign(ref $($mutability)* place,
                                          ref $($mutability)* rvalue) => {
                        self.visit_assign(block, place, rvalue, location);
                    }
                    StatementKind::EndRegion(_) => {}
                    StatementKind::Validate(_, ref $($mutability)* places) => {
                        for operand in places {
                            self.visit_place(& $($mutability)* operand.place,
                                              PlaceContext::Validate, location);
                            self.visit_ty(& $($mutability)* operand.ty,
                                          TyContext::Location(location));
                        }
                    }
                    StatementKind::SetDiscriminant{ ref $($mutability)* place, .. } => {
                        self.visit_place(place, PlaceContext::Store, location);
                    }
                    StatementKind::StorageLive(ref $($mutability)* local) => {
                        self.visit_local(local, PlaceContext::StorageLive, location);
                    }
                    StatementKind::StorageDead(ref $($mutability)* local) => {
                        self.visit_local(local, PlaceContext::StorageDead, location);
                    }
                    StatementKind::InlineAsm { ref $($mutability)* outputs,
                                               ref $($mutability)* inputs,
                                               asm: _ } => {
                        for output in & $($mutability)* outputs[..] {
                            self.visit_place(output, PlaceContext::Store, location);
                        }
                        for input in & $($mutability)* inputs[..] {
                            self.visit_operand(input, location);
                        }
                    }
                    StatementKind::Nop => {}
                }
            }

            fn super_assign(&mut self,
                            _block: BasicBlock,
                            place: &$($mutability)* Place<'tcx>,
                            rvalue: &$($mutability)* Rvalue<'tcx>,
                            location: Location) {
                self.visit_place(place, PlaceContext::Store, location);
                self.visit_rvalue(rvalue, location);
            }

            fn super_terminator(&mut self,
                                block: BasicBlock,
                                terminator: &$($mutability)* Terminator<'tcx>,
                                location: Location) {
                let Terminator {
                    ref $($mutability)* source_info,
                    ref $($mutability)* kind,
                } = *terminator;

                self.visit_source_info(source_info);
                self.visit_terminator_kind(block, kind, location);
            }

            fn super_terminator_kind(&mut self,
                                     block: BasicBlock,
                                     kind: & $($mutability)* TerminatorKind<'tcx>,
                                     source_location: Location) {
                match *kind {
                    TerminatorKind::Goto { target } => {
                        self.visit_branch(block, target);
                    }

                    TerminatorKind::SwitchInt { ref $($mutability)* discr,
                                                ref $($mutability)* switch_ty,
                                                ref values,
                                                ref targets } => {
                        self.visit_operand(discr, source_location);
                        self.visit_ty(switch_ty, TyContext::Location(source_location));
                        for value in &values[..] {
                            self.visit_const_int(value, source_location);
                        }
                        for &target in targets {
                            self.visit_branch(block, target);
                        }
                    }

                    TerminatorKind::Resume |
                    TerminatorKind::Return |
                    TerminatorKind::GeneratorDrop |
                    TerminatorKind::Unreachable => {
                    }

                    TerminatorKind::Drop { ref $($mutability)* location,
                                           target,
                                           unwind } => {
                        self.visit_place(location, PlaceContext::Drop, source_location);
                        self.visit_branch(block, target);
                        unwind.map(|t| self.visit_branch(block, t));
                    }

                    TerminatorKind::DropAndReplace { ref $($mutability)* location,
                                                     ref $($mutability)* value,
                                                     target,
                                                     unwind } => {
                        self.visit_place(location, PlaceContext::Drop, source_location);
                        self.visit_operand(value, source_location);
                        self.visit_branch(block, target);
                        unwind.map(|t| self.visit_branch(block, t));
                    }

                    TerminatorKind::Call { ref $($mutability)* func,
                                           ref $($mutability)* args,
                                           ref $($mutability)* destination,
                                           cleanup } => {
                        self.visit_operand(func, source_location);
                        for arg in args {
                            self.visit_operand(arg, source_location);
                        }
                        if let Some((ref $($mutability)* destination, target)) = *destination {
                            self.visit_place(destination, PlaceContext::Call, source_location);
                            self.visit_branch(block, target);
                        }
                        cleanup.map(|t| self.visit_branch(block, t));
                    }

                    TerminatorKind::Assert { ref $($mutability)* cond,
                                             expected: _,
                                             ref $($mutability)* msg,
                                             target,
                                             cleanup } => {
                        self.visit_operand(cond, source_location);
                        self.visit_assert_message(msg, source_location);
                        self.visit_branch(block, target);
                        cleanup.map(|t| self.visit_branch(block, t));
                    }

                    TerminatorKind::Yield { ref $($mutability)* value,
                                              resume,
                                              drop } => {
                        self.visit_operand(value, source_location);
                        self.visit_branch(block, resume);
                        drop.map(|t| self.visit_branch(block, t));

                    }

                    TerminatorKind::FalseEdges { real_target, ref imaginary_targets } => {
                        self.visit_branch(block, real_target);
                        for target in imaginary_targets {
                            self.visit_branch(block, *target);
                        }
                    }
                }
            }

            fn super_assert_message(&mut self,
                                    msg: & $($mutability)* AssertMessage<'tcx>,
                                    location: Location) {
                match *msg {
                    AssertMessage::BoundsCheck {
                        ref $($mutability)* len,
                        ref $($mutability)* index
                    } => {
                        self.visit_operand(len, location);
                        self.visit_operand(index, location);
                    }
                    AssertMessage::Math(_) => {},
                    AssertMessage::GeneratorResumedAfterReturn => {},
                    AssertMessage::GeneratorResumedAfterPanic => {},
                }
            }

            fn super_rvalue(&mut self,
                            rvalue: & $($mutability)* Rvalue<'tcx>,
                            location: Location) {
                match *rvalue {
                    Rvalue::Use(ref $($mutability)* operand) => {
                        self.visit_operand(operand, location);
                    }

                    Rvalue::Repeat(ref $($mutability)* value,
                                   ref $($mutability)* length) => {
                        self.visit_operand(value, location);
                        self.visit_const_usize(length, location);
                    }

                    Rvalue::Ref(ref $($mutability)* r, bk, ref $($mutability)* path) => {
                        self.visit_region(r, location);
                        self.visit_place(path, PlaceContext::Borrow {
                            region: *r,
                            kind: bk
                        }, location);
                    }

                    Rvalue::Len(ref $($mutability)* path) => {
                        self.visit_place(path, PlaceContext::Inspect, location);
                    }

                    Rvalue::Cast(_cast_kind,
                                 ref $($mutability)* operand,
                                 ref $($mutability)* ty) => {
                        self.visit_operand(operand, location);
                        self.visit_ty(ty, TyContext::Location(location));
                    }

                    Rvalue::BinaryOp(_bin_op,
                                     ref $($mutability)* lhs,
                                     ref $($mutability)* rhs) |
                    Rvalue::CheckedBinaryOp(_bin_op,
                                     ref $($mutability)* lhs,
                                     ref $($mutability)* rhs) => {
                        self.visit_operand(lhs, location);
                        self.visit_operand(rhs, location);
                    }

                    Rvalue::UnaryOp(_un_op, ref $($mutability)* op) => {
                        self.visit_operand(op, location);
                    }

                    Rvalue::Discriminant(ref $($mutability)* place) => {
                        self.visit_place(place, PlaceContext::Inspect, location);
                    }

                    Rvalue::NullaryOp(_op, ref $($mutability)* ty) => {
                        self.visit_ty(ty, TyContext::Location(location));
                    }

                    Rvalue::Aggregate(ref $($mutability)* kind,
                                      ref $($mutability)* operands) => {
                        let kind = &$($mutability)* **kind;
                        match *kind {
                            AggregateKind::Array(ref $($mutability)* ty) => {
                                self.visit_ty(ty, TyContext::Location(location));
                            }
                            AggregateKind::Tuple => {
                            }
                            AggregateKind::Adt(_adt_def,
                                               _variant_index,
                                               ref $($mutability)* substs,
                                               _active_field_index) => {
                                self.visit_substs(substs, location);
                            }
                            AggregateKind::Closure(ref $($mutability)* def_id,
                                                   ref $($mutability)* closure_substs) => {
                                self.visit_def_id(def_id, location);
                                self.visit_closure_substs(closure_substs, location);
                            }
                            AggregateKind::Generator(ref $($mutability)* def_id,
                                                   ref $($mutability)* closure_substs,
                                                   ref $($mutability)* interior) => {
                                self.visit_def_id(def_id, location);
                                self.visit_closure_substs(closure_substs, location);
                                self.visit_generator_interior(interior, location);
                            }
                        }

                        for operand in operands {
                            self.visit_operand(operand, location);
                        }
                    }
                }
            }

            fn super_operand(&mut self,
                             operand: & $($mutability)* Operand<'tcx>,
                             location: Location) {
                match *operand {
                    Operand::Copy(ref $($mutability)* place) => {
                        self.visit_place(place, PlaceContext::Copy, location);
                    }
                    Operand::Move(ref $($mutability)* place) => {
                        self.visit_place(place, PlaceContext::Move, location);
                    }
                    Operand::Constant(ref $($mutability)* constant) => {
                        self.visit_constant(constant, location);
                    }
                }
            }

            fn super_place(&mut self,
                            place: & $($mutability)* Place<'tcx>,
                            context: PlaceContext<'tcx>,
                            location: Location) {
                match *place {
                    Place::Local(ref $($mutability)* local) => {
                        self.visit_local(local, context, location);
                    }
                    Place::Static(ref $($mutability)* static_) => {
                        self.visit_static(static_, context, location);
                    }
                    Place::Projection(ref $($mutability)* proj) => {
                        self.visit_projection(proj, context, location);
                    }
                }
            }

            fn super_static(&mut self,
                            static_: & $($mutability)* Static<'tcx>,
                            _context: PlaceContext<'tcx>,
                            location: Location) {
                let Static {
                    ref $($mutability)* def_id,
                    ref $($mutability)* ty,
                } = *static_;
                self.visit_def_id(def_id, location);
                self.visit_ty(ty, TyContext::Location(location));
            }

            fn super_projection(&mut self,
                                proj: & $($mutability)* PlaceProjection<'tcx>,
                                context: PlaceContext<'tcx>,
                                location: Location) {
                let Projection {
                    ref $($mutability)* base,
                    ref $($mutability)* elem,
                } = *proj;
                let context = if context.is_mutating_use() {
                    PlaceContext::Projection(Mutability::Mut)
                } else {
                    PlaceContext::Projection(Mutability::Not)
                };
                self.visit_place(base, context, location);
                self.visit_projection_elem(elem, context, location);
            }

            fn super_projection_elem(&mut self,
                                     proj: & $($mutability)* PlaceElem<'tcx>,
                                     _context: PlaceContext<'tcx>,
                                     location: Location) {
                match *proj {
                    ProjectionElem::Deref => {
                    }
                    ProjectionElem::Subslice { from: _, to: _ } => {
                    }
                    ProjectionElem::Field(_field, ref $($mutability)* ty) => {
                        self.visit_ty(ty, TyContext::Location(location));
                    }
                    ProjectionElem::Index(ref $($mutability)* local) => {
                        self.visit_local(local, PlaceContext::Copy, location);
                    }
                    ProjectionElem::ConstantIndex { offset: _,
                                                    min_length: _,
                                                    from_end: _ } => {
                    }
                    ProjectionElem::Downcast(_adt_def, _variant_index) => {
                    }
                }
            }

            fn super_local_decl(&mut self,
                                local: Local,
                                local_decl: & $($mutability)* LocalDecl<'tcx>) {
                let LocalDecl {
                    mutability: _,
                    ref $($mutability)* ty,
                    name: _,
                    ref $($mutability)* source_info,
                    internal: _,
                    ref $($mutability)* lexical_scope,
                    is_user_variable: _,
                } = *local_decl;

                self.visit_ty(ty, TyContext::LocalDecl {
                    local,
                    source_info: *source_info,
                });
                self.visit_source_info(source_info);
                self.visit_visibility_scope(lexical_scope);
            }

            fn super_visibility_scope(&mut self,
                                      _scope: & $($mutability)* VisibilityScope) {
            }

            fn super_branch(&mut self,
                            _source: BasicBlock,
                            _target: BasicBlock) {
            }

            fn super_constant(&mut self,
                              constant: & $($mutability)* Constant<'tcx>,
                              location: Location) {
                let Constant {
                    ref $($mutability)* span,
                    ref $($mutability)* ty,
                    ref $($mutability)* literal,
                } = *constant;

                self.visit_span(span);
                self.visit_ty(ty, TyContext::Location(location));
                self.visit_literal(literal, location);
            }

            fn super_literal(&mut self,
                             literal: & $($mutability)* Literal<'tcx>,
                             location: Location) {
                match *literal {
                    Literal::Value { ref $($mutability)* value } => {
                        self.visit_const(value, location);
                    }
                    Literal::Promoted { index: _ } => {}
                }
            }

            fn super_def_id(&mut self, _def_id: & $($mutability)* DefId) {
            }

            fn super_span(&mut self, _span: & $($mutability)* Span) {
            }

            fn super_source_info(&mut self, source_info: & $($mutability)* SourceInfo) {
                let SourceInfo {
                    ref $($mutability)* span,
                    ref $($mutability)* scope,
                } = *source_info;

                self.visit_span(span);
                self.visit_visibility_scope(scope);
            }

            fn super_ty(&mut self, _ty: & $($mutability)* Ty<'tcx>) {
            }

            fn super_region(&mut self, _region: & $($mutability)* ty::Region<'tcx>) {
            }

            fn super_const(&mut self, _const: & $($mutability)* &'tcx ty::Const<'tcx>) {
            }

            fn super_substs(&mut self, _substs: & $($mutability)* &'tcx Substs<'tcx>) {
            }

            fn super_generator_interior(&mut self,
                                    _interior: & $($mutability)* GeneratorInterior<'tcx>) {
            }

            fn super_closure_substs(&mut self,
                                    _substs: & $($mutability)* ClosureSubsts<'tcx>) {
            }

            fn super_const_int(&mut self, _const_int: &ConstInt) {
            }

            fn super_const_usize(&mut self, _const_usize: & $($mutability)* ConstUsize) {
            }

            // Convenience methods

            fn visit_location(&mut self, mir: & $($mutability)* Mir<'tcx>, location: Location) {
                let basic_block = & $($mutability)* mir[location.block];
                if basic_block.statements.len() == location.statement_index {
                    if let Some(ref $($mutability)* terminator) = basic_block.terminator {
                        self.visit_terminator(location.block, terminator, location)
                    }
                } else {
                    let statement = & $($mutability)*
                        basic_block.statements[location.statement_index];
                    self.visit_statement(location.block, statement, location)
                }
            }
        }
    }
}

make_mir_visitor!(Visitor,);
make_mir_visitor!(MutVisitor,mut);

/// Extra information passed to `visit_ty` and friends to give context
/// about where the type etc appears.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyContext {
    LocalDecl {
        /// The index of the local variable we are visiting.
        local: Local,

        /// The source location where this local variable was declared.
        source_info: SourceInfo,
    },

    /// The return type of the function.
    ReturnTy(SourceInfo),

    /// A type found at some location.
    Location(Location),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlaceContext<'tcx> {
    // Appears as LHS of an assignment
    Store,

    // Dest of a call
    Call,

    // Being dropped
    Drop,

    // Being inspected in some way, like loading a len
    Inspect,

    // Being borrowed
    Borrow { region: Region<'tcx>, kind: BorrowKind },

    // Used as base for another place, e.g. `x` in `x.y`.
    //
    // The `Mutability` argument specifies whether the projection is being performed in order to
    // (potentially) mutate the place. For example, the projection `x.y` is marked as a mutation
    // in these cases:
    //
    //     x.y = ...;
    //     f(&mut x.y);
    //
    // But not in these cases:
    //
    //     z = x.y;
    //     f(&x.y);
    Projection(Mutability),

    // Consumed as part of an operand
    Copy,
    Move,

    // Starting and ending a storage live range
    StorageLive,
    StorageDead,

    // Validation command
    Validate,
}

impl<'tcx> PlaceContext<'tcx> {
    /// Returns true if this place context represents a drop.
    pub fn is_drop(&self) -> bool {
        match *self {
            PlaceContext::Drop => true,
            _ => false,
        }
    }

    /// Returns true if this place context represents a storage live or storage dead marker.
    pub fn is_storage_marker(&self) -> bool {
        match *self {
            PlaceContext::StorageLive | PlaceContext::StorageDead => true,
            _ => false,
        }
    }

    /// Returns true if this place context represents a storage live marker.
    pub fn is_storage_live_marker(&self) -> bool {
        match *self {
            PlaceContext::StorageLive => true,
            _ => false,
        }
    }

    /// Returns true if this place context represents a storage dead marker.
    pub fn is_storage_dead_marker(&self) -> bool {
        match *self {
            PlaceContext::StorageDead => true,
            _ => false,
        }
    }

    /// Returns true if this place context represents a use that potentially changes the value.
    pub fn is_mutating_use(&self) -> bool {
        match *self {
            PlaceContext::Store | PlaceContext::Call |
            PlaceContext::Borrow { kind: BorrowKind::Mut, .. } |
            PlaceContext::Projection(Mutability::Mut) |
            PlaceContext::Drop => true,
            PlaceContext::Inspect |
            PlaceContext::Borrow { kind: BorrowKind::Shared, .. } |
            PlaceContext::Borrow { kind: BorrowKind::Unique, .. } |
            PlaceContext::Projection(Mutability::Not) |
            PlaceContext::Copy | PlaceContext::Move |
            PlaceContext::StorageLive | PlaceContext::StorageDead |
            PlaceContext::Validate => false,
        }
    }

    /// Returns true if this place context represents a use that does not change the value.
    pub fn is_nonmutating_use(&self) -> bool {
        match *self {
            PlaceContext::Inspect | PlaceContext::Borrow { kind: BorrowKind::Shared, .. } |
            PlaceContext::Borrow { kind: BorrowKind::Unique, .. } |
            PlaceContext::Projection(Mutability::Not) |
            PlaceContext::Copy | PlaceContext::Move => true,
            PlaceContext::Borrow { kind: BorrowKind::Mut, .. } | PlaceContext::Store |
            PlaceContext::Call | PlaceContext::Projection(Mutability::Mut) |
            PlaceContext::Drop | PlaceContext::StorageLive | PlaceContext::StorageDead |
            PlaceContext::Validate => false,
        }
    }

    pub fn is_use(&self) -> bool {
        self.is_mutating_use() || self.is_nonmutating_use()
    }
}
