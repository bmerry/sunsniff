use clap::Parser;
use etherparse::SlicedPacket;
use futures::stream;
use influxdb2::models::DataPoint;
use influxdb2::Client;
use pcap::{Capture, Device};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    /// Host for influxdb
    #[clap(long, default_value = "http://localhost:8086")]
    host: String,

    /// Organisation for influxdb
    #[clap(long, required = true)]
    org: String,

    /// Token for influxdb
    #[clap(long, required = true)]
    token: String,

    /// Bucket for influxdb
    #[clap(long, required = true)]
    bucket: String,

    /// Capture device
    #[clap(long, required = true)]
    device: String,

    /// Treat --device as a file rather than a device
    #[clap(long)]
    file: bool,

    /// Filter expression for pcap
    #[clap(long)]
    filter: Option<String>,
}

struct Field {
    offset: usize,
    group: &'static str,
    name: &'static str,
    scale: f64,
    bias: f64,
    unit: &'static str,
}

impl Field {
    const fn new(
        offset: usize,
        group: &'static str,
        name: &'static str,
        scale: f64,
        bias: f64,
        unit: &'static str,
    ) -> Field {
        return Field {
            offset,
            group,
            name,
            scale,
            bias,
            unit,
        };
    }

    const fn power(offset: usize, group: &'static str) -> Field {
        return Field::new(offset, group, "Power", 1.0, 0.0, "W");
    }

    const fn voltage(offset: usize, group: &'static str) -> Field {
        return Field::new(offset, group, "Voltage", 0.1, 0.0, "V");
    }

    const fn current(offset: usize, group: &'static str) -> Field {
        return Field::new(offset, group, "Current", 0.01, 0.0, "A");
    }

    const fn temperature_name(offset: usize, group: &'static str, name: &'static str) -> Field {
        return Field::new(offset, group, name, 0.1, -100.0, "°C");
    }

    const fn temperature(offset: usize, group: &'static str) -> Field {
        return Field::temperature_name(offset, group, "Temperature");
    }

    const fn frequency(offset: usize, group: &'static str) -> Field {
        return Field::new(offset, group, "Frequency", 0.01, 0.0, "Hz");
    }

    const fn energy(offset: usize, group: &'static str, name: &'static str) -> Field {
        // TODO: these are probably 32-bit values, but more investigation is
        // needed to figure out where the high bits live.
        return Field::new(offset, group, name, 0.1, 0.0, "kWh");
    }
}

const MAGIC_LENGTH: usize = 292;
const MAGIC_HEADER: u8 = 0xa5;
const FIELDS: &'static [Field] = &[
    Field::energy(70, "Battery", "Total charge"),
    Field::energy(74, "Battery", "Total discharge"),
    Field::energy(82, "Grid", "Total import"),
    Field::energy(88, "Grid", "Total export"),
    Field::frequency(84, "Grid"),
    Field::energy(96, "Load", "Total consumption"),
    Field::temperature_name(106, "Inverter", "DC Temperature"),
    Field::temperature_name(108, "Inverter", "AC Temperature"),
    Field::energy(118, "PV", "Total production"),
    Field::voltage(176, "Grid"),
    Field::power(216, "Grid"),
    Field::power(228, "Load"),
    Field::temperature(240, "Battery"),
    Field::new(244, "Battery", "SOC", 1.0, 0.0, "%"),
    Field::power(248, "PV"),
    Field::power(256, "Battery"),
    Field::current(258, "Battery"),
];

async fn run<T: pcap::Activated>(
    cap: &mut Capture<T>,
    client: &Client,
    args: &Args,
) -> Result<(), Box<dyn std::error::Error>> {
    let base_filter = "tcp";
    let filter = match &args.filter {
        Some(expr) => format!("({}) and ({})", base_filter, expr),
        None => String::from(base_filter),
    };
    cap.filter(filter.as_str(), true)?;
    cap.set_datalink(pcap::Linktype::ETHERNET)?;

    loop {
        match cap.next_packet() {
            Ok(packet) => {
                let sliced = SlicedPacket::from_ethernet(packet.data)?;
                if sliced.payload.len() == MAGIC_LENGTH && sliced.payload[0] == MAGIC_HEADER {
                    let serial = std::str::from_utf8(&sliced.payload[11..21]).unwrap_or("unknown");
                    let timestamp = (packet.header.ts.tv_sec as i64) * 1000000000i64
                        + (packet.header.ts.tv_usec as i64) * 1000i64;
                    let mut points = vec![];
                    for field in FIELDS.iter() {
                        let bytes = &sliced.payload[field.offset..field.offset + 2];
                        let bytes = <&[u8; 2]>::try_from(bytes)?;
                        let value = i16::from_be_bytes(*bytes);
                        let value = (value as f64) * field.scale + field.bias;
                        println!(
                            "{} {} {}: {} {}",
                            serial, field.group, field.name, value, field.unit
                        );
                        points.push(
                            DataPoint::builder("inverter")
                                .timestamp(timestamp)
                                .tag("serial", serial)
                                .tag("group", field.group)
                                .tag("name", field.name)
                                .tag("unit", field.unit)
                                .field("value", value)
                                .build()?,
                        );
                    }
                    println!("");
                    client.write(&args.bucket, stream::iter(points)).await?;
                }
            }
            Err(pcap::Error::TimeoutExpired) => {}
            Err(err) => Err(err)?,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let client = Client::new(&args.host, &args.org, &args.token);
    // Check that we're at least able to connect to the server
    // TODO: actually check that the server is healthy
    client.health().await?;

    if args.file {
        let mut cap = Capture::from_file(&args.device)?;
        run(&mut cap, &client, &args).await?;
    } else {
        let device = Device::from(args.device.as_str());
        let mut cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        run(&mut cap, &client, &args).await?;
    }
    Ok(())
}
