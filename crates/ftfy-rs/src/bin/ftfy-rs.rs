//! The `ftfy-rs` command: fix the text in a file. It takes the same options
//! as Python ftfy's `ftfy` command and writes the same output, but fixes
//! lines on all CPU cores.

use std::fs::File;
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use ftfy_rs::codecs::{self, Codec};
use ftfy_rs::{fix_and_explain, Normalization, TextFixerConfig, UnescapeHtml};

const NAME: &str = "ftfy-rs";

const USAGE: &str = "usage: ftfy-rs [-h] [-o OUTPUT] [-g] [-e ENCODING] [-n NORMALIZATION]
               [--preserve-entities]
               [filename]";

const HELP: &str = "
positional arguments:
  filename              The file whose Unicode is to be fixed. Defaults to -,
                        meaning standard input.

options:
  -h, --help            show this help message and exit
  -o, --output OUTPUT   The file to output to. Defaults to -, meaning standard
                        output.
  -g, --guess           Ask ftfy to guess the encoding of your input. This is
                        risky. Overrides -e.
  -e, --encoding ENCODING
                        The encoding of the input. Defaults to UTF-8.
  -n, --normalization NORMALIZATION
                        The normalization of Unicode to apply. Defaults to
                        NFC. Can be \"none\".
  --preserve-entities   Leave HTML entities as they are. The default is to
                        decode them, as long as no HTML tags have appeared in
                        the file.";

// These match the messages of ftfy's command.
const DECODE_ERROR_TEXT: &str = "ftfy error:
This input couldn't be decoded as {encoding}. We got the following error:

    {error}

ftfy works best when its input is in a known encoding. You can use `ftfy -g`
to guess, if you're desperate. Otherwise, give the encoding name with the
`-e` option, such as `ftfy -e latin-1`.
";

const SAME_FILE_ERROR_TEXT: &str = "ftfy error:
Can't read and write the same file. Please output to a new file instead.
";

/// Read this much input at a time, then fix it in parallel.
const BATCH_BYTES: usize = 4 << 20;
/// Below this much text, fix it on one thread.
const PARALLEL_MIN_BYTES: usize = 64 << 10;

struct Args {
    filename: String,
    output: String,
    guess: bool,
    encoding: String,
    normalization: String,
    preserve_entities: bool,
}

enum ParseResult {
    Run(Args),
    Help,
    Error(String),
}

const OPTIONS: [(&str, Option<&str>, bool); 6] = [
    // (long name, short name, takes a value)
    ("--output", Some("-o"), true),
    ("--guess", Some("-g"), false),
    ("--encoding", Some("-e"), true),
    ("--normalization", Some("-n"), true),
    ("--preserve-entities", None, false),
    ("--help", Some("-h"), false),
];

/// Parse arguments the way Python's argparse does for ftfy's command,
/// including `--opt=value`, `-ovalue` and unambiguous abbreviations.
fn parse_args(raw: Vec<String>) -> ParseResult {
    let mut args = Args {
        filename: "-".into(),
        output: "-".into(),
        guess: false,
        encoding: "utf-8".into(),
        normalization: "NFC".into(),
        preserve_entities: false,
    };
    let mut positional = Vec::new();
    let mut unrecognized = Vec::new();
    let mut iter = raw.into_iter();
    let mut only_positional = false;

    while let Some(arg) = iter.next() {
        if only_positional || arg == "-" || !arg.starts_with('-') {
            positional.push(arg);
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }

        // Find the option, and any value attached to it.
        let (option, attached) = if arg.starts_with("--") {
            let (name, value) = match arg.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (arg.as_str(), None),
            };
            let matches: Vec<_> = OPTIONS
                .iter()
                .filter(|(long, _, _)| long.starts_with(name))
                .collect();
            match matches.as_slice() {
                [only] => (*only, value),
                [] => {
                    unrecognized.push(arg.clone());
                    continue;
                }
                many => {
                    let names: Vec<_> = many.iter().map(|(long, _, _)| *long).collect();
                    return ParseResult::Error(format!(
                        "ambiguous option: {name} could match {}",
                        names.join(", ")
                    ));
                }
            }
        } else {
            let short = &arg[..2];
            match OPTIONS.iter().find(|(_, s, _)| *s == Some(short)) {
                Some(option) => {
                    let rest = &arg[2..];
                    (option, (!rest.is_empty()).then(|| rest.to_string()))
                }
                None => {
                    unrecognized.push(arg.clone());
                    continue;
                }
            }
        };

        let (long, short, takes_value) = *option;
        let label = match short {
            Some(short) => format!("{short}/{long}"),
            None => long.to_string(),
        };
        let value = if takes_value {
            match attached.or_else(|| {
                let next = iter.as_slice().first()?;
                (next == "-" || !next.starts_with('-')).then(|| iter.next().unwrap())
            }) {
                Some(value) => value,
                None => {
                    return ParseResult::Error(format!("argument {label}: expected one argument"))
                }
            }
        } else if attached.is_some() && long.len() > 2 && arg.starts_with("--") {
            return ParseResult::Error(format!("argument {label}: ignored explicit argument"));
        } else {
            String::new()
        };

        match long {
            "--output" => args.output = value,
            "--guess" => args.guess = true,
            "--encoding" => args.encoding = value,
            "--normalization" => args.normalization = value,
            "--preserve-entities" => args.preserve_entities = true,
            "--help" => return ParseResult::Help,
            _ => unreachable!(),
        }
    }

    let mut positional = positional.into_iter();
    if let Some(filename) = positional.next() {
        args.filename = filename;
    }
    unrecognized.extend(positional);
    if !unrecognized.is_empty() {
        return ParseResult::Error(format!(
            "unrecognized arguments: {}",
            unrecognized.join(" ")
        ));
    }
    ParseResult::Run(args)
}

/// Like Python's os.path.realpath: resolve what exists, and make the rest
/// absolute.
fn real_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    path.canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Print a message to stderr, with line endings like Python's.
fn print_error(message: &str) {
    let _ = write_text(&mut io::stderr().lock(), message);
}

fn fail(message: &str) -> ExitCode {
    print_error(message);
    ExitCode::from(1)
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1).collect()) {
        ParseResult::Run(args) => args,
        ParseResult::Help => {
            let help = format!(
                "{USAGE}\n\nftfy-rs (fixes text for you, in Rust), version {}\n{HELP}\n",
                env!("CARGO_PKG_VERSION")
            );
            let _ = write_text(&mut io::stdout().lock(), &help);
            return ExitCode::SUCCESS;
        }
        ParseResult::Error(message) => {
            print_error(&format!("{USAGE}\n{NAME}: error: {message}\n"));
            return ExitCode::from(2);
        }
    };

    let codec = if args.guess {
        None
    } else {
        match Codec::lookup(&args.encoding) {
            Some(codec) => Some(codec),
            None => {
                return fail(&format!(
                    "ftfy-rs error:\nThe encoding {:?} isn't one that the native ftfy-rs command \
                     supports.\nThe Python version, `python -m ftfy_rs.cli`, supports every \
                     encoding Python does.\n",
                    args.encoding
                ))
            }
        }
    };
    let normalization = if args.normalization.eq_ignore_ascii_case("none") {
        None
    } else {
        match Normalization::from_name(&args.normalization) {
            Some(form) => Some(form),
            None => return fail("ftfy-rs error:\ninvalid normalization form\n"),
        }
    };

    let input: Box<dyn Read> = if args.filename == "-" {
        Box::new(io::stdin().lock())
    } else {
        match File::open(&args.filename) {
            Ok(file) => Box::new(file),
            Err(err) => {
                return fail(&format!(
                    "ftfy-rs error:\nCan't open {:?}: {err}\n",
                    args.filename
                ))
            }
        }
    };
    let output: Box<dyn Write> = if args.output == "-" {
        Box::new(io::stdout().lock())
    } else {
        if real_path(&args.output) == real_path(&args.filename) {
            return fail(SAME_FILE_ERROR_TEXT);
        }
        match File::create(&args.output) {
            Ok(file) => Box::new(file),
            Err(err) => {
                return fail(&format!(
                    "ftfy-rs error:\nCan't write {:?}: {err}\n",
                    args.output
                ))
            }
        }
    };

    let mut fixer = Fixer {
        codec,
        unescape_html: if args.preserve_entities {
            UnescapeHtml::Never
        } else {
            UnescapeHtml::Auto
        },
        normalization,
        // Python's error message shows the encoding that was asked for,
        // which is None when guessing.
        encoding_repr: if args.guess {
            "None".to_string()
        } else {
            format!("'{}'", args.encoding)
        },
    };
    match fixer.run(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Decode(message)) => fail(&message),
        // Like most command-line tools, stop quietly if the reader of our
        // output goes away, as with `ftfy-rs file | head`.
        Err(Failure::Io(err)) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::from(1),
        Err(Failure::Io(err)) => fail(&format!("ftfy-rs error:\n{err}\n")),
    }
}

enum Failure {
    Decode(String),
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(err: io::Error) -> Self {
        Failure::Io(err)
    }
}

/// Fixes input line by line, as ftfy's `fix_file` does.
struct Fixer {
    /// None until the first line is guessed, with -g.
    codec: Option<Codec>,
    unescape_html: UnescapeHtml,
    normalization: Option<Normalization>,
    encoding_repr: String,
}

impl Fixer {
    fn run(&mut self, input: Box<dyn Read>, output: Box<dyn Write>) -> Result<(), Failure> {
        let mut reader = BufReader::with_capacity(BATCH_BYTES, input);
        let mut writer = BufWriter::with_capacity(1 << 20, output);
        let mut batch: Vec<Vec<u8>> = Vec::new();
        let mut batch_bytes = 0;
        loop {
            let mut line = Vec::new();
            let eof = reader.read_until(b'\n', &mut line)? == 0;
            if !eof {
                batch_bytes += line.len();
                batch.push(line);
            }
            // Fix what we have when the batch is big, or when no more input
            // is ready without waiting, so interactive use stays responsive.
            if eof || batch_bytes >= BATCH_BYTES || reader.buffer().is_empty() {
                self.fix_batch(&batch, &mut writer)?;
                writer.flush()?;
                batch.clear();
                batch_bytes = 0;
            }
            if eof {
                return Ok(());
            }
        }
    }

    fn fix_batch(&mut self, lines: &[Vec<u8>], out: &mut impl Write) -> Result<(), Failure> {
        // Decoding and the HTML check depend on earlier lines, so they go in
        // order. A decoding error stops the output at the line before it.
        let mut texts = Vec::with_capacity(lines.len());
        let mut decode_error = None;
        for line in lines {
            let decoded = match self.codec {
                Some(codec) => codec.decode(line),
                None => codecs::guess(line).map(|(text, codec)| {
                    self.codec = Some(codec);
                    text
                }),
            };
            match decoded {
                Ok(text) => {
                    if self.unescape_html == UnescapeHtml::Auto && text.contains('<') {
                        self.unescape_html = UnescapeHtml::Never;
                    }
                    texts.push((text, self.unescape_html));
                }
                Err(err) => {
                    decode_error = Some(err);
                    break;
                }
            }
        }

        let config = TextFixerConfig {
            normalization: self.normalization,
            ..TextFixerConfig::no_explain()
        };
        let fix = |(text, unescape_html): &(String, UnescapeHtml)| {
            let config = TextFixerConfig {
                unescape_html: *unescape_html,
                ..config.clone()
            };
            fix_and_explain(text, &config).0
        };
        for fixed in parallel_map(&texts, fix) {
            write_text(out, &fixed)?;
        }

        match decode_error {
            None => Ok(()),
            Some(err) => {
                out.flush()?;
                Err(Failure::Decode(
                    DECODE_ERROR_TEXT
                        .replace("{encoding}", &self.encoding_repr)
                        .replace("{error}", &err.to_string()),
                ))
            }
        }
    }
}

/// Write text the way Python's text-mode files do, which on Windows turn
/// "\n" into "\r\n".
fn write_text(out: &mut impl Write, text: &str) -> io::Result<()> {
    if cfg!(windows) {
        let mut parts = text.split('\n');
        if let Some(first) = parts.next() {
            out.write_all(first.as_bytes())?;
        }
        for part in parts {
            out.write_all(b"\r\n")?;
            out.write_all(part.as_bytes())?;
        }
        Ok(())
    } else {
        out.write_all(text.as_bytes())
    }
}

/// Fix each line, in order, splitting the work across threads when there's
/// enough of it. Threads take small chunks of lines from a shared counter, so
/// a few long lines don't leave the others idle.
fn parallel_map<F>(lines: &[(String, UnescapeHtml)], f: F) -> Vec<String>
where
    F: Fn(&(String, UnescapeHtml)) -> String + Sync,
{
    let total_bytes: usize = lines.iter().map(|(text, _)| text.len()).sum();
    let threads = thread::available_parallelism().map_or(1, |n| n.get());
    if total_bytes < PARALLEL_MIN_BYTES || threads == 1 {
        return lines.iter().map(f).collect();
    }

    let chunks: Vec<_> = lines.chunks(lines.len().div_ceil(threads * 8)).collect();
    let next = AtomicUsize::new(0);
    let mut done: Vec<(usize, Vec<String>)> = thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(chunk) = chunks.get(i) else {
                            return mine;
                        };
                        mine.push((i, chunk.iter().map(&f).collect()));
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("fixing text doesn't panic"))
            .collect()
    });
    done.sort_unstable_by_key(|(i, _)| *i);
    done.into_iter().flat_map(|(_, fixed)| fixed).collect()
}
