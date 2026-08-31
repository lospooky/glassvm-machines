use glassvm_core::SinkError;

pub(super) fn sink_error(error: SinkError) -> String {
    format!("emission sink rejected PICO-8 run data: {error}")
}
