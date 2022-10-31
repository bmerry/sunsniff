use async_trait::async_trait;
use futures::channel::mpsc::UnboundedReceiver;
use std::sync::Arc;

#[derive(Debug)]
pub struct Field<'a> {
    pub offset: usize,
    pub group: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    pub scale: f64,
    pub bias: f64,
    pub unit: &'a str,
}

#[derive(Debug)]
pub struct Update<'a> {
    pub timestamp: i64, // Nanoseconds since UNIX epoch
    pub serial: String,
    pub fields: &'a [Field<'a>],
    pub values: Vec<f64>,
}

#[async_trait]
pub trait Receiver {
    async fn run<'a>(&mut self, receiver: UnboundedReceiver<Arc<Update<'a>>>);
}

impl<'a> Field<'a> {
    pub const fn new(
        offset: usize,
        group: &'a str,
        name: &'a str,
        id: &'a str,
        scale: f64,
        bias: f64,
        unit: &'a str,
    ) -> Field {
        return Field {
            offset,
            group,
            name,
            id,
            scale,
            bias,
            unit,
        };
    }

    pub const fn power(offset: usize, group: &'a str, id: &'a str) -> Field {
        return Field::new(offset, group, "Power", id, 1.0, 0.0, "W");
    }

    pub const fn voltage(offset: usize, group: &'a str, id: &'a str) -> Field {
        return Field::new(offset, group, "Voltage", id, 0.1, 0.0, "V");
    }

    pub const fn current(offset: usize, group: &'a str, id: &'a str) -> Field {
        return Field::new(offset, group, "Current", id, 0.01, 0.0, "A");
    }

    pub const fn temperature_name(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Field {
        return Field::new(offset, group, name, id, 0.1, -100.0, "°C");
    }

    pub const fn temperature(offset: usize, group: &'a str, id: &'a str) -> Field {
        return Field::temperature_name(offset, group, "Temperature", id);
    }

    pub const fn frequency(offset: usize, group: &'a str, id: &'a str) -> Field {
        return Field::new(offset, group, "Frequency", id, 0.01, 0.0, "Hz");
    }

    pub const fn energy(offset: usize, group: &'a str, name: &'a str, id: &'a str) -> Field {
        // TODO: these are probably 32-bit values, but more investigation is
        // needed to figure out where the high bits live.
        return Field::new(offset, group, name, id, 0.1, 0.0, "kWh");
    }
}

impl<'a> Update<'a> {
    pub fn new(
        timestamp: i64,
        serial: impl Into<String>,
        fields: &'a [Field<'a>],
        values: Vec<f64>,
    ) -> Update<'a> {
        return Update {
            timestamp,
            serial: serial.into(),
            fields,
            values,
        };
    }
}
