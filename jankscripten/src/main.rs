use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use clap::Clap;

#[derive(Clap)]
struct Compile {
    #[clap(short, long)]
    output: Option<String>,
    input: String
}

#[derive(Clap)]
struct Run {
    input: String
}

#[derive(Clap)]
enum SubCommand {
    Compile(Compile),
    Run(Run)
}

#[derive(Clap)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
}

fn make_output_filename(
    opt_output: &Option<String>,
    input_path: &Path,
    default_extension: &str) -> PathBuf {
    match opt_output {
        Some(name) => PathBuf::from(name),
        None => {
            input_path.with_extension(default_extension)
        }
    }
}

fn read_file(path: &Path) -> String {
    match fs::read_to_string(path) {
        Err(err) => {
            eprintln!("Error reading from {}\n{}", path.to_string_lossy(), err);
            process::exit(1);
        }
        Ok(s) => s
    }
}

fn compile_notwasm(input: &str, output: &Path) {
    use libjankscripten::notwasm;
    let wasm = notwasm::compile(notwasm::parse(input)).expect("compile error");
    fs::write(output, wasm).expect("writing file");
}

fn compile(opts: Compile) {
    let input_path = Path::new(&opts.input);
    match input_path.extension() {
        None => {
            eprintln!("Input filename does not have an extension.");
            process::exit(1);
        }
        Some(ext) => {
            let ext = ext.to_str().expect("filename extension is not UTF-8");
            match ext {
                "notwasm" => {
                    let output_path = make_output_filename(&opts.output, input_path, "wasm");
                    let input = read_file(input_path);
                    compile_notwasm(&input, output_path.as_path());
                }
                _ => {
                    eprintln!("Unsupported extension: .{}", ext);
                    process::exit(1);
                }
            }
        }
    }
}

fn run(opts: Run) {
    // TODO(arjun): Should we call tester, or just fold it into this binary?
    unimplemented!();
}

fn main() {
    let opts = Opts::parse();
    match opts.subcmd {
        SubCommand::Compile(opts) => compile(opts),
        SubCommand::Run(opts) => run(opts)
    }
}
