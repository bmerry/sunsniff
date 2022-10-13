use clap::Parser;
use etherparse::SlicedPacket;
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

    /// Capture device
    #[clap(long, required=true)]
    device: String,

    /// Treat --device as a file rather than a device
    #[clap(long)]
    file: bool,
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

const FIELDS: &'static [Field] = &[
    Field::new(228, "Load", 1.0, "W"),
    Field::new(244, "SoC", 1.0, "%"),
];

async fn run<T: pcap::Activated>(cap: &mut Capture<T>, args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    cap.filter("tcp", true)?;
    cap.set_datalink(pcap::Linktype::ETHERNET)?;

    loop {
        match cap.next_packet() {
            Ok(packet) => {
                let sliced = SlicedPacket::from_ethernet(packet.data)?;
                if sliced.payload.len() == 292 {
                    for field in FIELDS.iter() {
                        let bytes = &sliced.payload[field.offset..field.offset + 2];
                        let bytes = <&[u8; 2]>::try_from(bytes)?;
                        let value = i16::from_be_bytes(*bytes);
                        let value = (value as f64) * field.scale;
                        println!("{}: {} {}", field.name, value, field.unit);
                    }
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
    let health = client.health().await?;

    if args.file {
        let mut cap = Capture::from_file(&args.device)?;
        run(&mut cap, &args).await?;
    } else {
        let device = Device::from(args.device.as_str());
        let mut cap = Capture::from_device(device)?.immediate_mode(true).open()?;
        run(&mut cap, &args).await?;
    }
    Ok(())
}
