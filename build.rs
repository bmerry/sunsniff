use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Duplicate of crate::fields::FieldType
#[derive(Deserialize, Debug)]
enum FieldType {
    Charge,
    Current,
    Energy,
    Frequency,
    Power,
    StateOfCharge,
    Temperature,
    Voltage,
}

use FieldType::*;

#[derive(Deserialize)]
struct Record {
    field_type: FieldType,
    group: String,
    name: String,
    id: String,
    scale: Option<f64>,
    offset: Option<u32>,
    #[allow(dead_code)]
    reg: Option<u16>,
    #[allow(dead_code)]
    reg2: Option<u16>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("field_definitions.rs");
    let mut reader = csv::Reader::from_reader(fs::File::open("fields.csv")?);
    let mut writer = fs::File::create(dest_path)?;
    writeln!(&mut writer, "/// Fields found in each packet")?;
    writeln!(&mut writer, "pub const FIELDS: &[Field] = &[")?;
    for result in reader.deserialize() {
        let record: Record = result?;
        let default_scale = match record.field_type {
            Charge | Power | StateOfCharge => Some(1.0),
            Energy | Temperature => Some(0.1),
            Frequency => Some(0.01),
            Current | Voltage => None,
        };
        let bias = match record.field_type {
            Temperature => -100.0,
            _ => 0.0,
        };
        let unit = match record.field_type {
            Charge => "Ah",
            Current => "A",
            Energy => "kWh",
            Frequency => "Hz",
            Power => "W",
            StateOfCharge => "%",
            Temperature => "°C",
            Voltage => "V",
        };
        let scale = record.scale.or(default_scale).unwrap();
        if let Some(offset) = record.offset {
            writeln!(&mut writer, r#"    Field {{
        field_type: FieldType::{:?},
        offset: {offset},
        group: {:?},
        name: {:?},
        id: {:?},
        scale: {scale:?},
        bias: {bias:?},
        unit: {unit:?},
    }},"#, record.field_type, record.group, record.name, record.id)?;
        }
    }
    writeln!(&mut writer, "];")?;
    drop(writer);
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=fields.csv");
    Ok(())
}
