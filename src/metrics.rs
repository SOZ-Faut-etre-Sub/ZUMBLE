use lazy_static::lazy_static;
use prometheus::{opts, register_int_counter_vec, register_int_gauge};
use prometheus::{IntCounterVec, IntGauge};

lazy_static! {
    pub static ref MESSAGES_TOTAL: IntCounterVec = register_int_counter_vec!(
        opts!("zumble_messages_total", "number of messages"),
        &["protocol", "direction", "kind"]
    )
    .expect("can't create a metric");
    pub static ref MESSAGES_BYTES: IntCounterVec =
        register_int_counter_vec!(opts!("zumble_messages_bytes", "message bytes"), &["protocol", "direction", "kind"])
            .expect("can't create a metric");
    pub static ref CLIENTS_TOTAL: IntGauge =
        register_int_gauge!(opts!("zumble_clients_total", "Total number of clients")).expect("can't create a metric");


    pub static ref UNKNOWN_MESSAGES_TOTAL: IntCounterVec = register_int_counter_vec!(
        opts!("zumble_unknown_messages_total", "number of unknown messages (sent from clients not initialized)"),
        &["protocol", "direction", "kind"]
    )
    .expect("can't create a metric");

    pub static ref UNKNOWN_MESSAGES_BYTES: IntCounterVec =
        register_int_counter_vec!(opts!("zumble_unknown_messages_bytes", "unknown message bytes (sent from clients not initialized)"), &["protocol", "direction", "kind"])
            .expect("can't create a metric");
}
