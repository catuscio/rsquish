use clap::{Parser, Subcommand, ValueEnum};
use raster_core::{Mode, Request};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Parser)]
#[command(
    name = "rsquish",
    version,
    about = "Local image optimization with measured quality and size constraints"
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Encode and measure real candidates; never overwrite input or existing output.
    Optimize {
        /// JPEG, PNG or WebP files. Multiple inputs use a bounded worker pool.
        #[arg(required = true)]
        input: Vec<PathBuf>,
        #[arg(short, long)]
        output_dir: PathBuf,
        #[arg(long, value_enum, default_value = "target")]
        mode: Objective,
        #[arg(long, default_value_t = 85.0)]
        target_score: f64,
        /// Per-image byte ceiling for budget mode (decimal bytes).
        #[arg(long, default_value_t = 200_000)]
        max_bytes: u64,
        #[arg(long)]
        max_width: Option<u32>,
        #[arg(long, value_delimiter = ',', default_value = "webp,avif-iq,avif-ssim")]
        codecs: Vec<String>,
        /// Maximum actual encode probes per codec, including coarse 50/75/95.
        #[arg(long, default_value_t = 7)]
        probes: u8,
        /// Soft wall-time budget per image; running codec/metric completes first.
        #[arg(long, default_value_t = 120)]
        max_seconds: u64,
        /// Images processed at once: auto, 1, 2, 3, or 4.
        #[arg(long, default_value = "auto", value_parser = ["auto", "1", "2", "3", "4"])]
        jobs: String,
        /// Suppress progress on stderr. stdout always contains a JSON result array.
        #[arg(long)]
        quiet: bool,
    },
    /// Show compiled-in capabilities; no external encoders are needed at runtime.
    Capabilities,
}
#[derive(Clone, Copy, ValueEnum)]
enum Objective {
    Target,
    Budget,
}

fn main() {
    let args = Args::parse();
    match args.command {
        Command::Capabilities => {
            println!(
                "{}",
                serde_json::json!({"version":env!("CARGO_PKG_VERSION"),
                "codecs":raster_core::CODECS,"metric":raster_core::METRIC,
                "inputs":["jpeg","png","webp"],"opaque8BitOnly":true,
                "runtimeExternalEncoders":false,"maximumInputPixels":32_000_000,
                "metadata":"normalize RGB ICC to sRGB; apply EXIF orientation; strip metadata",
                "qualityGuarantee":"measured threshold only; not visually lossless"})
            );
        }
        Command::Optimize {
            input,
            output_dir,
            mode,
            target_score,
            max_bytes,
            max_width,
            codecs,
            probes,
            max_seconds,
            quiet,
            jobs,
        } => {
            let stop = Arc::new(AtomicBool::new(false));
            let signal = stop.clone();
            if let Err(e) = ctrlc::set_handler(move || {
                signal.store(true, Ordering::Relaxed);
            }) {
                eprintln!("Cannot install Ctrl-C handler: {e}");
                std::process::exit(1);
            }
            let mut results = vec![];
            let mut exit = 0;
            let requests: Vec<_> = input
                .iter()
                .map(|path| Request {
                    input_path: path.clone(),
                    output_dir: output_dir.clone(),
                    mode: match mode {
                        Objective::Target => Mode::Target,
                        Objective::Budget => Mode::Budget,
                    },
                    target_score,
                    max_bytes,
                    max_width,
                    codecs: codecs.clone(),
                    probes,
                    max_seconds,
                })
                .collect();
            let requested = if jobs == "auto" {
                0
            } else {
                jobs.parse().unwrap()
            };
            let plan =
                raster_core::plan_batch(&requests, requested).expect("validated worker count");
            if !quiet {
                eprintln!(
                    "Concurrent images: {}{}",
                    plan.jobs,
                    if plan.automatic { " (auto)" } else { "" }
                );
            }
            let batch = raster_core::optimize_batch(
                &requests,
                plan.jobs,
                &stop,
                |index, p| {
                    if !quiet {
                        eprintln!(
                            "[{}/{}] {}: {}",
                            p.completed,
                            p.total,
                            input[index].display(),
                            p.message
                        );
                    }
                },
                |_, _| {},
            )
            .expect("validated worker count");
            for (index, result) in batch {
                let path = &input[index];
                match result {
                    Ok(report) => {
                        if report.status == "cancelled" {
                            exit = 130;
                        } else if report.status != "saved" && exit == 0 {
                            exit = 2;
                        }
                        results.push(serde_json::to_value(report).unwrap());
                    }
                    Err(error) => {
                        exit = 1;
                        results.push(
                            serde_json::json!({"inputPath":path,"status":"error","error":error}),
                        );
                    }
                }
            }
            if stop.load(Ordering::Relaxed) {
                exit = 130;
            }
            println!("{}", serde_json::to_string_pretty(&results).unwrap());
            std::process::exit(exit);
        }
    }
}
