//! RTL-CDC-001 (UnsynchronizedCdcCrossing): Signal driven in one clock domain
//! sampled directly in another clock domain without a synchronizer.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unsynchronized_cdc_crossing(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).cdc_crossings {
        let detail = if hit.width > 1 {
            format!("multi-bit ({}-bit) bus '{}'", hit.width, hit.src_reg)
        } else if hit.combinational {
            format!("1-bit signal '{}' through combinational logic", hit.src_reg)
        } else {
            format!("1-bit signal '{}' without a 2-FF synchronizer chain", hit.src_reg)
        };
        out.add(
            "RTL-CDC-001",
            "UnsynchronizedCdcCrossing",
            Severity::Warning,
            format!(
                "Unsynchronized CDC crossing in module '{}': {} driven by clock '{}' is sampled in clock domain '{}'. \
Use a dedicated 2-FF synchronizer for single-bit controls or a Gray-coded/handshake FIFO for multi-bit buses.",
                hit.module_name, detail, hit.src_clk, hit.dst_clk
            ),
            hit.loc,
        );
    }
}
