use syntax::ast::*;
use rustc::lint::{Context, LintPass, LintArray, Lint};
use syntax::codemap::Span;
use syntax::visit::{Visitor, FnKind, walk_ty};
use utils::{in_external_macro, span_lint};
use std::collections::HashSet;
use std::iter::FromIterator;

declare_lint!(pub NEEDLESS_LIFETIMES, Warn,
              "using explicit lifetimes for references in function arguments when elision rules \
               would allow omitting them");

#[derive(Copy,Clone)]
pub struct LifetimePass;

impl LintPass for LifetimePass {
    fn get_lints(&self) -> LintArray {
        lint_array!(NEEDLESS_LIFETIMES)
    }

    fn check_fn(&mut self, cx: &Context, kind: FnKind, decl: &FnDecl,
                _: &Block, span: Span, _: NodeId) {
        if in_external_macro(cx, span) {
            return;
        }
        if could_use_elision(kind, decl) {
            span_lint(cx, NEEDLESS_LIFETIMES, span,
                      "explicit lifetimes given in parameter types where they could be elided");
        }
    }
}

/// The lifetime of a &-reference.
#[derive(PartialEq, Eq, Hash, Debug)]
enum RefLt {
    Unnamed,
    Static,
    Named(Name),
}
use self::RefLt::*;

fn could_use_elision(kind: FnKind, func: &FnDecl) -> bool {
    // There are two scenarios where elision works:
    // * no output references, all input references have different LT
    // * output references, exactly one input reference with same LT

    // these will collect all the lifetimes for references in arg/return types
    let mut input_visitor = RefVisitor(Vec::new());
    let mut output_visitor = RefVisitor(Vec::new());

    // extract lifetime in "self" argument for methods (there is a "self" argument
    // in func.inputs, but its type is TyInfer)
    if let FnKind::FkMethod(_, sig, _) = kind {
        match sig.explicit_self.node {
            SelfRegion(ref opt_lt, _, _) => input_visitor.record(opt_lt),
            SelfExplicit(ref ty, _) => walk_ty(&mut input_visitor, ty),
            _ => { }
        }
    }
    // extract lifetimes in input argument types
    for arg in &func.inputs {
        walk_ty(&mut input_visitor, &*arg.ty);
    }
    // extract lifetimes in output type
    if let Return(ref ty) = func.output {
        walk_ty(&mut output_visitor, ty);
    }

    let input_lts = input_visitor.into_vec();
    let output_lts = output_visitor.into_vec();

    // no input lifetimes? easy case!
    if input_lts.is_empty() {
        return false;
    } else if output_lts.is_empty() {
        // no output lifetimes, check distinctness of input lifetimes

        // only one reference with unnamed lifetime, ok
        if input_lts.len() == 1 && input_lts[0] == Unnamed {
            return false;
        }
        // we have no output reference, so we only need all distinct lifetimes
        if input_lts.len() == unique_lifetimes(&input_lts) {
            return true;
        }
    } else {
        // we have output references, so we need one input reference,
        // and all output lifetimes must be the same
        if unique_lifetimes(&output_lts) > 1 {
            return false;
        }
        if input_lts.len() == 1 {
            match (&input_lts[0], &output_lts[0]) {
                (&Named(n1), &Named(n2)) if n1 == n2 => { return true; }
                (&Named(_), &Unnamed) => { return true; }
                (&Unnamed, &Named(_)) => { return true; }
                _ => { } // already elided, different named lifetimes
                         // or something static going on
            }
        }
    }
    false
}

/// Number of unique lifetimes in the given vector.
fn unique_lifetimes(lts: &Vec<RefLt>) -> usize {
    lts.iter().collect::<HashSet<_>>().len()
}

/// A visitor usable for syntax::visit::walk_ty().
struct RefVisitor(Vec<RefLt>);

impl RefVisitor {
    fn record(&mut self, lifetime: &Option<Lifetime>) {
        if let &Some(ref lt) = lifetime {
            if lt.name.as_str() == "'static" {
                self.0.push(Static);
            } else {
                self.0.push(Named(lt.name));
            }
        } else {
            self.0.push(Unnamed);
        }
    }

    fn into_vec(self) -> Vec<RefLt> {
        self.0
    }
}

impl<'v> Visitor<'v> for RefVisitor {
    // for lifetimes of references
    fn visit_opt_lifetime_ref(&mut self, _: Span, lifetime: &'v Option<Lifetime>) {
        self.record(lifetime);
    }

    // for lifetimes as parameters of generics
    fn visit_lifetime_ref(&mut self, lifetime: &'v Lifetime) {
        self.record(&Some(*lifetime));
    }

    // for lifetime bounds; the default impl calls visit_lifetime_ref
    fn visit_lifetime_bound(&mut self, _: &'v Lifetime) { }
}