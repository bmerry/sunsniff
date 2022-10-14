use clap::Parser;
use etherparse::SlicedPacket;
use futures::stream;
use influxdb2::Client;
use influxdb2::models::DataPoint;
use pcap::{Capture, Device};

#[derive(Debug, Parser)]
#[clap(author, version)]
struct Args {
    /// Host for influxdb
    #[clap(long, default_value="http://localhost:8086")]
    host: String,

    /// Organisation for influxdb
    #[clap(long, required=true)]
    org: String,

    /// Token for influxdb
    #[clap(long, required=true)]
    token: String,

    /// Bucket for influxdb
    #[clap(long, required=true)]
    bucket: String,

    /// Capture device
    #[clap(long, required=true)]
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
    name: &'static str,
    scale: f64,
    unit: &'static str,
}

impl Field {
    const fn new(offset: usize, name: &'static str, scale: f64, unit: &'static str) -> Field {
        return Field { offset, name, scale, unit };
    }
}

const MAGIC_LENGTH: usize = 292;
const FIELDS: &'static [Field] = &[
    Field::new(228, "Load", 1.0, "W"),
    Field::new(244, "SoC", 1.0, "%"),
];

async fn run<T: pcap::Activated>(cap: &mut Capture<T>, client: &Client, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
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
                if sliced.payload.len() == MAGIC_LENGTH {
                    let timestamp = (packet.header.ts.tv_sec as i64) * 1000000000i64 + (packet.header.ts.tv_usec as i64) * 1000i64;
                    let mut points = vec![];
                    for field in FIELDS.iter() {
                        let bytes = &sliced.payload[field.offset..field.offset + 2];
                        let bytes = <&[u8; 2]>::try_from(bytes)?;
                        let value = i16::from_be_bytes(*bytes);
                        let value = (value as f64) * field.scale;
                        println!("{}: {} {}", field.name, value, field.unit);
                        points.push(
                            DataPoint::builder("power")
                                .timestamp(timestamp)
                                .tag("name", field.name)
                                .tag("unit", field.unit)
                                .field("value", value)
                                .build()?
                        );
                    }
                    client.write(&args.bucket, stream::iter(points)).await?;
                }
            },
            Err(pcap::Error::TimeoutExpired) => {},
            Err(err) => { Err(err)? },
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
