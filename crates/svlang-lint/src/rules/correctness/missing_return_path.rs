//! SVL-COR-015 (MissingReturnPath): Non-void function has a control-flow path
//! that reaches end-of-function without returning a value.

use crate::support::cfg_dataflow;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn missing_return_path(c: &Cst, out: &mut Sink) {
    for hit in &cfg_dataflow::facts(c).missing_returns {
        out.add(
            "SVL-COR-015",
            "MissingReturnPath",
            Severity::Warning,
            format!(
                "Non-void function '{}' (return type '{}') has a control-flow path that falls through without a return statement. \
Falling off a non-void function returns the uninitialized default of '{}'.",
                hit.func_name, hit.ret_type, hit.ret_type
            ),
            hit.loc,
        );
    }
}
