// Rustacity: A Digital Audio Editor
// Copyright (C) 2026 Sadik Pranto
// SPDX-License-Identifier: GPL-2.0-or-later

//! Runs one extension over one WAV file, without the editor.
//!
//! This is the third way to drive the extension host, beside the integration
//! tests and the C++ effect: a person can point it at an extension directory
//! and a WAV file and hear what the extension did. That makes it the tool for
//! checking a new extension before it is packaged, and for seeing what a
//! misbehaving one looks like from the outside - `hostile-loop` here is a
//! five-second wall-clock wait and an error, not a hung process.
//!
//! It runs exactly as the editor does: one instance for the whole file, blocks
//! of `--block` frames (4096 by default, the ABI's 65536 at most), parameters
//! set once before the first block. Whatever is not 32-bit float is scaled to
//! it on the way in; the output is always written as 32-bit float, so a
//! comparison against a reference is exact rather than rounded twice.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use rustacity_ext::{
    abi, Engine, ExtError, Extension, HostConfig, Instance, Parameter, ParameterKind,
};

/// How many frames the extension is handed at once unless asked otherwise.
const DEFAULT_BLOCK: usize = 4096;

const USAGE: &str = "\
rustacity-ext-cli - run a Rustacity extension over a WAV file

Usage:
  rustacity-ext-cli [options] <extension-dir> <input.wav> <output.wav>
  rustacity-ext-cli [options] --describe <extension-dir>

Options:
  --param <id>=<value>  set a parameter before the first block (repeatable);
                        a switch takes true/false, a choice its index or name
  --block <frames>      most frames to hand the extension at once, 1..65536
                        (default 4096)
  --describe            print what the extension is and what it has, then stop
  --verbose, -v         print each parameter as it is set, and what each call
                        cost
  --help, -h            this text
  --version             this tool's version and the API version it speaks

One instance of the extension runs the whole file, in the sandbox described in
docs/extensions/SANDBOX.md.  The output is written as 32-bit float, whatever
the input was.
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Fault::Usage(message)) => {
            eprintln!("rustacity-ext-cli: {message}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Fault::Failed(message)) => {
            eprintln!("rustacity-ext-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Something to tell the user about. A usage mistake and a failure exit
/// differently, so they are not the same thing here either.
enum Fault {
    /// The command line does not make sense.
    Usage(String),
    /// The command line made sense; something went wrong carrying it out.
    Failed(String),
}

fn usage(message: impl Into<String>) -> Fault {
    Fault::Usage(message.into())
}

fn failed(message: impl Into<String>) -> Fault {
    Fault::Failed(message.into())
}

/// A host error, as the user should see it: the API's own words, which are
/// written to be read.
fn host(error: ExtError) -> Fault {
    Fault::Failed(error.to_string())
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

struct Options {
    /// The extension to load.
    extension: PathBuf,
    /// The WAV to read, and the WAV to write. Absent with `--describe`.
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    /// Parameter id and the text the user wrote, resolved against the manifest
    /// once the manifest is known.
    parameters: Vec<(String, String)>,
    block: usize,
    describe: bool,
    verbose: bool,
}

enum Request {
    Help,
    Version,
    Run(Box<Options>),
}

fn parse(arguments: &[String]) -> Result<Request, Fault> {
    let mut options = Options {
        extension: PathBuf::new(),
        input: None,
        output: None,
        parameters: Vec::new(),
        block: DEFAULT_BLOCK,
        describe: false,
        verbose: false,
    };
    let mut positional: Vec<String> = Vec::new();

    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        match argument {
            "--help" | "-h" => return Ok(Request::Help),
            "--version" => return Ok(Request::Version),
            "--verbose" | "-v" => options.verbose = true,
            "--describe" => options.describe = true,
            "--param" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| usage("--param needs an id=value after it"))?;
                let (id, text) = value.split_once('=').ok_or_else(|| {
                    usage(format!("'{value}' is not id=value; --param gain_db=6"))
                })?;
                if id.is_empty() {
                    return Err(usage("--param needs a parameter id before the '='"));
                }
                options.parameters.push((id.to_string(), text.to_string()));
            }
            "--block" => {
                index += 1;
                let value = arguments
                    .get(index)
                    .ok_or_else(|| usage("--block needs a frame count after it"))?;
                let frames: usize = value
                    .parse()
                    .map_err(|_| usage(format!("'{value}' is not a number of frames")))?;
                if frames == 0 || frames > abi::MAX_BLOCK_FRAMES {
                    return Err(usage(format!(
                        "a block of {frames} frames is outside 1..{}",
                        abi::MAX_BLOCK_FRAMES
                    )));
                }
                options.block = frames;
            }
            other if other.starts_with('-') => {
                return Err(usage(format!("there is no option '{other}'")))
            }
            other => positional.push(other.to_string()),
        }
        index += 1;
    }

    let mut positional = positional.into_iter();
    options.extension = PathBuf::from(
        positional
            .next()
            .ok_or_else(|| usage("an extension directory is needed"))?,
    );
    if options.describe {
        if let Some(extra) = positional.next() {
            return Err(usage(format!(
                "--describe takes only the extension directory, not '{extra}'"
            )));
        }
    } else {
        options.input = Some(PathBuf::from(
            positional
                .next()
                .ok_or_else(|| usage("a WAV file to read is needed"))?,
        ));
        options.output = Some(PathBuf::from(
            positional
                .next()
                .ok_or_else(|| usage("a WAV file to write is needed"))?,
        ));
        if let Some(extra) = positional.next() {
            return Err(usage(format!("unexpected argument '{extra}'")));
        }
    }

    Ok(Request::Run(Box::new(options)))
}

fn run(arguments: &[String]) -> Result<(), Fault> {
    match parse(arguments)? {
        Request::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Request::Version => {
            println!(
                "rustacity-ext-cli {} (extension API version {})",
                env!("CARGO_PKG_VERSION"),
                abi::API_VERSION
            );
            Ok(())
        }
        Request::Run(options) => process(&options),
    }
}

// ---------------------------------------------------------------------------
// Running an extension
// ---------------------------------------------------------------------------

/// What one run did, for the summary at the end.
struct Stats {
    frames: usize,
    calls: usize,
    /// Wall-clock time spent inside the extension, summed over its calls.
    inside: Duration,
    /// The slowest single call.
    slowest: Duration,
}

fn process(options: &Options) -> Result<(), Fault> {
    let engine = Engine::new(HostConfig::default()).map_err(host)?;
    let extension = engine.load(&options.extension).map_err(host)?;

    if options.describe {
        describe(&extension);
        return Ok(());
    }

    let input_path = options.input.clone().expect("parse checked this");
    let output_path = options.output.clone().expect("parse checked this");

    let mut reader = WavReader::open(&input_path)
        .map_err(|error| failed(format!("cannot read {}: {error}", input_path.display())))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err(failed(format!(
            "{} says it has no channels",
            input_path.display()
        )));
    }
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    // Always 32-bit float out: a comparison against a reference then depends
    // only on the extension, not on how the file was written.
    let output_spec = WavSpec {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let writer = WavWriter::create(&output_path, output_spec)
        .map_err(|error| failed(format!("cannot write {}: {error}", output_path.display())))?;

    let mut instance = extension
        .instantiate(sample_rate as f32, options.block, channels)
        .map_err(host)?;
    apply_parameters(&mut instance, &extension, options)?;

    if options.verbose {
        println!(
            "{}: {} frame(s) of {} channel(s) at {} Hz, {} frames per call",
            extension.name(),
            reader.duration(),
            channels,
            sample_rate,
            options.block
        );
    }

    let outcome = run_blocks(&mut instance, &mut reader, writer, spec, options);
    // Whatever happened, what the extension logged on the way is worth having.
    print_logs(&mut instance);
    let stats = outcome?;
    let inside = milliseconds(stats.inside);
    let slowest = milliseconds(stats.slowest);
    println!(
        "{} frame(s), {} call(s) -> {} ('{}' {})",
        stats.frames,
        stats.calls,
        output_path.display(),
        extension.name(),
        extension.version()
    );
    println!("{inside:.3} ms inside the extension, slowest call {slowest:.3} ms");
    Ok(())
}

/// Sets every `--param`, through the manifest's own rules, so that a switch
/// takes true/false, a choice its name, and a value outside the range is
/// refused with the range in the message.
fn apply_parameters(
    instance: &mut Instance,
    extension: &Extension,
    options: &Options,
) -> Result<(), Fault> {
    for (id, text) in &options.parameters {
        let parameter = extension.manifest().parameter(id).ok_or_else(|| {
            let known: Vec<&str> = extension
                .parameters()
                .iter()
                .map(|parameter| parameter.id.as_str())
                .collect();
            failed(format!(
                "'{id}' is not a parameter of this extension; it has: {}",
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ))
        })?;
        let value = parameter.parse_value(text).map_err(host)?;
        instance.set_param_by_id(id, value).map_err(host)?;
        if options.verbose {
            println!("{id} = {value}");
        }
    }
    Ok(())
}

/// Reads the file a block at a time and writes what comes back, in whatever
/// sample type the file holds.
fn run_blocks(
    instance: &mut Instance,
    reader: &mut WavReader<std::io::BufReader<std::fs::File>>,
    writer: WavWriter<std::io::BufWriter<std::fs::File>>,
    spec: WavSpec,
    options: &Options,
) -> Result<Stats, Fault> {
    match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 8) => {
            pump::<i8>(instance, reader, writer, spec, integer_scale(8), options)
        }
        (SampleFormat::Int, 16) => {
            pump::<i16>(instance, reader, writer, spec, integer_scale(16), options)
        }
        (SampleFormat::Int, 24) => {
            pump::<i32>(instance, reader, writer, spec, integer_scale(24), options)
        }
        (SampleFormat::Int, 32) => {
            pump::<i32>(instance, reader, writer, spec, integer_scale(32), options)
        }
        (SampleFormat::Float, 32) => pump::<f32>(instance, reader, writer, spec, 1.0, options),
        (format, bits) => Err(failed(format!(
            "a {bits}-bit {format:?} WAV is not something this tool reads; \
             convert the file to 16-bit integer or 32-bit float"
        ))),
    }
}

/// The scale that turns this many-bit integer samples into the -1..1 the ABI
/// carries.
fn integer_scale(bits: u32) -> f64 {
    1.0 / (1u64 << (bits - 1)) as f64
}

/// The block loop, over one sample type.
fn pump<S>(
    instance: &mut Instance,
    reader: &mut WavReader<std::io::BufReader<std::fs::File>>,
    mut writer: WavWriter<std::io::BufWriter<std::fs::File>>,
    spec: WavSpec,
    scale: f64,
    options: &Options,
) -> Result<Stats, Fault>
where
    S: hound::Sample + Copy + Into<f64>,
{
    let channels = spec.channels as usize;
    let block = options.block;
    let mut input: Vec<Vec<f32>> = vec![vec![0.0; block]; channels];
    let mut output: Vec<Vec<f32>> = vec![vec![0.0; block]; channels];

    let mut samples = reader.samples::<S>();
    let mut stats = Stats {
        frames: 0,
        calls: 0,
        inside: Duration::ZERO,
        slowest: Duration::ZERO,
    };

    loop {
        // Fill one block, or as much of one as the file has left. A file that
        // ends part way through a frame reads as if it ended at the frame.
        let mut frames = 0;
        let mut read_error: Option<hound::Error> = None;
        'fill: while frames < block {
            for row in input.iter_mut() {
                match samples.next() {
                    Some(Ok(sample)) => row[frames] = (sample.into() * scale) as f32,
                    Some(Err(error)) => {
                        read_error = Some(error);
                        break 'fill;
                    }
                    None => break 'fill,
                }
            }
            frames += 1;
        }

        if let Some(error) = read_error {
            // The writer is dropped, which writes the file's real length into
            // its header: what was read before the error is still a playable
            // file, and the error below is what says it is not the whole one.
            return Err(failed(format!(
                "cannot read the input after {} frame(s): {error}",
                stats.frames
            )));
        }
        if frames == 0 {
            return finish(writer, stats);
        }

        let input_slices: Vec<&[f32]> = input.iter().map(|channel| &channel[..frames]).collect();
        let mut output_slices: Vec<&mut [f32]> = output
            .iter_mut()
            .map(|channel| &mut channel[..frames])
            .collect();

        let started = Instant::now();
        let result = instance.process(&input_slices, &mut output_slices);
        let elapsed = started.elapsed();
        result.map_err(host)?;

        stats.calls += 1;
        stats.inside += elapsed;
        stats.slowest = stats.slowest.max(elapsed);
        if options.verbose {
            println!(
                "call {:>4}: {frames:>6} frames in {:.3} ms",
                stats.calls,
                milliseconds(elapsed)
            );
        }

        // The ABI is planar and hound wants whole frames, so this takes one
        // sample from each channel per frame, in channel order.
        for (frame, _) in output[0][..frames].iter().enumerate() {
            for row in &output {
                writer
                    .write_sample(row[frame])
                    .map_err(|error| failed(format!("cannot write the output: {error}")))?;
            }
        }
        stats.frames += frames;
        if frames < block {
            return finish(writer, stats);
        }
    }
}

/// Closes the output: this is what writes the real length into its header,
/// which is the one thing hound cannot do for itself in the happy path.
fn finish(
    writer: WavWriter<std::io::BufWriter<std::fs::File>>,
    stats: Stats,
) -> Result<Stats, Fault> {
    writer
        .finalize()
        .map(|()| stats)
        .map_err(|error| failed(format!("cannot finish the output file: {error}")))
}

/// The extension's own log, which is where it says what it was doing.
fn print_logs(instance: &mut Instance) {
    let records = instance.take_logs();
    for record in &records {
        println!("[{}] {}", record.level_name(), record.message);
    }
    let dropped = instance.dropped_log_records();
    if dropped > 0 {
        println!("({dropped} log record(s) dropped: rx_log is rate-limited)");
    }
}

/// What the extension says about itself, for `--describe`.
fn describe(extension: &Extension) {
    println!("{} {}", extension.name(), extension.version());
    println!("  id:      {}", extension.id());
    println!("  loaded:  {}", extension.path().display());
    if let Some(description) = extension.manifest().description.as_deref() {
        println!("  about:   {description}");
    }
    let capabilities = &extension.manifest().capabilities;
    println!(
        "  asks:    {}",
        if capabilities.is_empty() {
            "(nothing)".to_string()
        } else {
            capabilities.join(", ")
        }
    );

    let parameters = extension.parameters();
    if parameters.is_empty() {
        println!("  parameters: none");
        return;
    }
    println!("  parameters:");
    for parameter in parameters {
        println!(
            "    [{}] {:<14} {:<20} {}",
            parameter.index,
            parameter.id,
            parameter.name,
            kind_of(parameter)
        );
        if let Some(description) = parameter.description.as_deref() {
            println!("        {description}");
        }
    }
}

/// One parameter's kind, range, choices and default, in one line.
fn kind_of(parameter: &Parameter) -> String {
    let unit = parameter
        .unit
        .as_deref()
        .map(|unit| format!(" {unit}"))
        .unwrap_or_default();
    match &parameter.kind {
        ParameterKind::Float { min, max } => {
            format!("float {min}..{max}{unit}, default {}", parameter.default)
        }
        ParameterKind::Bool => format!("bool (true/false), default {}", parameter.default != 0.0),
        ParameterKind::Choice { choices } => {
            let chosen = choices
                .get(parameter.default as usize)
                .map(String::as_str)
                .unwrap_or("?");
            format!(
                "choice 0..{} {}, default {} ('{chosen}')",
                choices.len() - 1,
                choices.join("/"),
                parameter.default
            )
        }
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
