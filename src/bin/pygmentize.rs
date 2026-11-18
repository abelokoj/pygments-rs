//! `pygmentize`-compatible CLI backed purely by `pygments-rs`.

use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;

use pygments_rs::{
    Filter, LexOptions, Lexer, PYGMENTS_VERSION, all_lexers, apply_filters, filter_by_name, filter_names,
    format_string, formatter_by_name, formatter_for_filename, formatter_names, guess_lexer, lexer_by_name,
    lexer_for_filename, style_names,
};

#[derive(Default)]
struct Args {
    lexer: Option<String>,
    guess: bool,
    guess_filename: Option<String>,
    guess_stdin: bool,
    formatter: Option<String>,
    style_defs: Option<String>,
    style_arg: Option<String>,
    filters: Vec<String>,
    options: Vec<(String, String)>,
    list: Option<Vec<String>>,
    help_item: Option<(String, String)>,
    output: Option<String>,
    streaming: bool,
    version: bool,
    help: bool,
    input: Option<String>,
}

fn parse_comma_opts(raw: &str, out: &mut Vec<(String, String)>) {
    for part in raw.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.push((k.trim().to_owned(), v.trim().to_owned()));
        } else {
            out.push((trimmed.to_owned(), "true".to_owned()));
        }
    }
}

fn take_opt_val(args: &[String], i: &mut usize, flag: &str, rest: &str) -> Result<String, String> {
    if !rest.is_empty() {
        return Ok(rest.to_owned());
    }
    *i += 1;
    args.get(*i).cloned().ok_or_else(|| format!("option {flag} requires an argument"))
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut out = Args::default();
    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "-h" | "--help" => out.help = true,
            "-v" | "-V" | "--version" => out.version = true,
            "-g" => out.guess = true,
            "-C" => out.guess_stdin = true,
            "-s" => out.streaming = true,
            "-L" => {
                let mut cats = Vec::new();
                while let Some(next) = raw.get(i + 1) {
                    if next.starts_with('-') {
                        break;
                    }
                    cats.push(next.clone());
                    i += 1;
                }
                out.list = Some(cats);
            }
            "-H" => {
                let what = raw.get(i + 1).ok_or("-H requires <kind> <name>")?.clone();
                let name = raw.get(i + 2).ok_or("-H requires <kind> <name>")?.clone();
                i += 2;
                out.help_item = Some((what, name));
            }
            _ if arg.starts_with("-l") => {
                out.lexer = Some(take_opt_val(&raw, &mut i, "-l", &arg[2..])?);
            }
            _ if arg.starts_with("-N") => {
                out.guess_filename = Some(take_opt_val(&raw, &mut i, "-N", &arg[2..])?);
            }
            _ if arg.starts_with("-f") => {
                out.formatter = Some(take_opt_val(&raw, &mut i, "-f", &arg[2..])?);
            }
            _ if arg.starts_with("-S") => {
                out.style_defs = Some(take_opt_val(&raw, &mut i, "-S", &arg[2..])?);
            }
            _ if arg.starts_with("-a") => {
                out.style_arg = Some(take_opt_val(&raw, &mut i, "-a", &arg[2..])?);
            }
            _ if arg.starts_with("-F") => {
                out.filters.push(take_opt_val(&raw, &mut i, "-F", &arg[2..])?);
            }
            _ if arg.starts_with("-O") => {
                let v = take_opt_val(&raw, &mut i, "-O", &arg[2..])?;
                parse_comma_opts(&v, &mut out.options);
            }
            _ if arg.starts_with("-P") => {
                let v = take_opt_val(&raw, &mut i, "-P", &arg[2..])?;
                if let Some((k, val)) = v.split_once('=') {
                    out.options.push((k.trim().to_owned(), val.to_owned()));
                } else {
                    out.options.push((v.trim().to_owned(), "true".to_owned()));
                }
            }
            _ if arg.starts_with("-o") => {
                out.output = Some(take_opt_val(&raw, &mut i, "-o", &arg[2..])?);
            }
            "--" => {
                if let Some(file) = raw.get(i + 1) {
                    out.input = Some(file.clone());
                }
                break;
            }
            s if s.starts_with('-') && s != "-" => {
                return Err(format!("unknown option {s:?}"));
            }
            _ => {
                if out.input.is_some() {
                    return Err(format!("unexpected extra input argument {arg:?}"));
                }
                out.input = Some(arg.clone());
            }
        }
        i += 1;
    }
    Ok(out)
}

fn print_usage() {
    println!(
        "Usage: pygmentize [-l <lexer> | -g] [-F <filter[:opts]>] [-f <formatter>]
                  [-O <options>] [-P <option=value>] [-s] [-o <outfile>] [<infile>]
       pygmentize -S <style> -f <formatter> [-a <arg>] [-O <options>]
       pygmentize -L [lexers | formatters | filters | styles]
       pygmentize -N <filename>
       pygmentize -C
       pygmentize -H <lexer|formatter|filter> <name>
       pygmentize -V | -h"
    );
}

fn print_list(cats: &[String]) -> Result<(), String> {
    let want_all = cats.is_empty();
    let targets: Vec<&str> = if want_all {
        vec!["lexer", "formatter", "filter", "style"]
    } else {
        cats.iter().map(|s| s.trim_end_matches('s')).collect()
    };
    for &t in &targets {
        match t {
            "lexer" => {
                println!("\nLexers:\n~~~~~~~");
                for l in all_lexers() {
                    if l.aliases().is_empty() {
                        continue;
                    }
                    let files = if l.filenames().is_empty() {
                        String::new()
                    } else {
                        format!(" (filenames {})", l.filenames().join(", "))
                    };
                    println!("* {}:\n    {}{files}", l.aliases().join(", "), l.name());
                }
            }
            "formatter" => {
                println!("\nFormatters:\n~~~~~~~~~~~");
                for &name in formatter_names() {
                    if let Ok(f) = formatter_by_name(name, &[]) {
                        let files = if f.filenames().is_empty() {
                            String::new()
                        } else {
                            format!(" (filenames {})", f.filenames().join(", "))
                        };
                        println!("* {}:\n    {}{files}", f.aliases().join(", "), f.name());
                    }
                }
            }
            "filter" => {
                println!("\nFilters:\n~~~~~~~~");
                for &name in filter_names() {
                    println!("* {name}");
                }
            }
            "style" => {
                println!("\nStyles:\n~~~~~~~");
                for name in style_names() {
                    println!("* {name}");
                }
            }
            other => return Err(format!("unknown category {other:?} (expected lexer, formatter, filter, or style)")),
        }
    }
    Ok(())
}

fn print_help_item(what: &str, name: &str) -> Result<(), String> {
    match what {
        "lexer" => {
            let l = lexer_by_name(name).ok_or_else(|| format!("lexer {name:?} not found"))?;
            println!("Lexer: {} ({})", l.name(), l.class_name());
            println!("Aliases: {}", l.aliases().join(", "));
            if !l.filenames().is_empty() {
                println!("Filenames: {}", l.filenames().join(", "));
            }
            if !l.mimetypes().is_empty() {
                println!("MIME types: {}", l.mimetypes().join(", "));
            }
        }
        "formatter" => {
            let f = formatter_by_name(name, &[]).map_err(|e| e.0)?;
            println!("Formatter: {}", f.name());
            println!("Aliases: {}", f.aliases().join(", "));
            if !f.filenames().is_empty() {
                println!("Filenames: {}", f.filenames().join(", "));
            }
        }
        "filter" => {
            filter_by_name(name, &[]).map_err(|e| e.0)?;
            println!("Filter: {name}");
        }
        _ => return Err(format!("unknown help topic {what:?} (expected lexer, formatter, or filter)")),
    }
    Ok(())
}

fn build_filters(specs: &[String]) -> Result<Vec<Box<dyn Filter>>, String> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let (name, raw_opts) = spec.split_once(':').unwrap_or((spec, ""));
        let mut owned = Vec::new();
        if !raw_opts.is_empty() {
            parse_comma_opts(raw_opts, &mut owned);
        }
        let pairs: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        out.push(filter_by_name(name, &pairs).map_err(|e| e.0)?);
    }
    Ok(out)
}

fn parse_bool_str(s: &str) -> Result<bool, String> {
    match s.to_ascii_lowercase().as_str() {
        "1" | "yes" | "true" | "on" => Ok(true),
        "0" | "no" | "false" | "off" => Ok(false),
        _ => Err(format!("invalid boolean {s:?}")),
    }
}

fn lex_options_for(lexer: Lexer, opts: &[(String, String)]) -> Result<LexOptions, String> {
    let mut lo = lexer.default_options();
    for (k, v) in opts {
        match k.as_str() {
            "stripnl" => lo.stripnl = parse_bool_str(v)?,
            "stripall" => lo.stripall = parse_bool_str(v)?,
            "ensurenl" => lo.ensurenl = parse_bool_str(v)?,
            "tabsize" => {
                lo.tabsize = v.parse().map_err(|_| format!("invalid tabsize {v:?}"))?;
            }
            _ => {}
        }
    }
    Ok(lo)
}

fn read_input(path: Option<&str>) -> Result<String, String> {
    match path {
        Some(p) if p != "-" => fs::read_to_string(p).map_err(|e| format!("cannot read {p:?}: {e}")),
        _ => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s).map_err(|e| format!("cannot read stdin: {e}"))?;
            Ok(s)
        }
    }
}

fn write_output(path: Option<&str>, content: &str) -> Result<(), String> {
    match path {
        Some(p) => fs::write(p, content).map_err(|e| format!("cannot write {p:?}: {e}")),
        None => {
            let mut stdout = io::stdout().lock();
            stdout
                .write_all(content.as_bytes())
                .and_then(|()| stdout.flush())
                .map_err(|e| format!("cannot write stdout: {e}"))
        }
    }
}

fn run() -> Result<(), ExitCode> {
    let args = parse_args().map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(2)
    })?;

    if args.help {
        print_usage();
        return Ok(());
    }
    if args.version {
        println!("pygmentize (pygments-rs {}, Pygments {PYGMENTS_VERSION})", env!("CARGO_PKG_VERSION"));
        if args.list.is_none() {
            return Ok(());
        }
    }
    if let Some(cats) = &args.list {
        print_list(cats).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(2)
        })?;
        return Ok(());
    }
    if let Some((what, name)) = &args.help_item {
        print_help_item(what, name).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(1)
        })?;
        return Ok(());
    }
    if let Some(fname) = &args.guess_filename {
        let alias = lexer_for_filename(fname, None).and_then(|l| l.aliases().first().copied()).unwrap_or("text");
        println!("{alias}");
        return Ok(());
    }
    if args.guess_stdin {
        let content = read_input(args.input.as_deref()).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(1)
        })?;
        let alias = guess_lexer(&content).and_then(|l| l.aliases().first().copied()).unwrap_or("text");
        println!("{alias}");
        return Ok(());
    }

    let mut fmt_opts_owned = args.options.clone();
    if let Some(style) = &args.style_defs {
        let Some(fmt_name) = args.formatter.as_deref() else {
            eprintln!("pygmentize: -S requires -f <formatter>");
            return Err(ExitCode::from(2));
        };
        fmt_opts_owned.push(("style".into(), style.clone()));
        let pairs: Vec<(&str, &str)> = fmt_opts_owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        let fmt = formatter_by_name(fmt_name, &pairs).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(1)
        })?;
        let defs = fmt.style_defs(args.style_arg.as_deref().unwrap_or(""));
        let mut content = defs;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        write_output(args.output.as_deref(), &content).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(1)
        })?;
        return Ok(());
    }

    let pairs: Vec<(&str, &str)> = fmt_opts_owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let formatter = match (&args.formatter, &args.output) {
        (Some(name), _) => formatter_by_name(name, &pairs),
        (None, Some(out)) => formatter_for_filename(out, &pairs),
        (None, None) => formatter_by_name("terminal", &pairs),
    }
    .map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(1)
    })?;

    let filters = build_filters(&args.filters).map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(1)
    })?;
    let filter_refs: Vec<&dyn Filter> = filters.iter().map(|f| f.as_ref()).collect();

    if args.streaming {
        let Some(name) = args.lexer.as_deref() else {
            eprintln!("pygmentize: -s requires -l <lexer>");
            return Err(ExitCode::from(2));
        };
        let lexer = lexer_by_name(name).ok_or_else(|| {
            eprintln!("pygmentize: no lexer for alias {name:?}");
            ExitCode::from(1)
        })?;
        let stdin = io::stdin();
        let mut file_out = match args.output.as_deref() {
            Some(p) => Some(fs::File::create(p).map_err(|e| {
                eprintln!("pygmentize: cannot create {p:?}: {e}");
                ExitCode::from(1)
            })?),
            None => None,
        };
        for line in stdin.lock().lines() {
            let mut line = line.map_err(|e| {
                eprintln!("pygmentize: read error: {e}");
                ExitCode::from(1)
            })?;
            line.push('\n');
            let filtered = apply_filters(Some(lexer), lexer.tokenize(&line), &filter_refs).map_err(|e| {
                eprintln!("pygmentize: {e}");
                ExitCode::from(1)
            })?;
            let rendered = format_string(filtered.iter().map(|(ty, s)| (*ty, s.as_ref())), formatter.as_ref());
            if let Some(ref mut f) = file_out {
                f.write_all(rendered.as_bytes()).and_then(|()| f.flush()).map_err(|e| {
                    eprintln!("pygmentize: write error: {e}");
                    ExitCode::from(1)
                })?;
            } else {
                let mut stdout = io::stdout().lock();
                stdout.write_all(rendered.as_bytes()).and_then(|()| stdout.flush()).map_err(|e| {
                    eprintln!("pygmentize: write error: {e}");
                    ExitCode::from(1)
                })?;
            }
        }
        return Ok(());
    }

    let code = read_input(args.input.as_deref()).map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(1)
    })?;
    let lexer = if let Some(name) = args.lexer.as_deref() {
        lexer_by_name(name).ok_or_else(|| {
            eprintln!("pygmentize: no lexer for alias {name:?}");
            ExitCode::from(1)
        })?
    } else if args.guess {
        guess_lexer(&code).or_else(|| lexer_by_name("text")).ok_or(ExitCode::from(1))?
    } else if let Some(path) = args.input.as_deref().filter(|p| *p != "-") {
        lexer_for_filename(path, Some(&code)).or_else(|| lexer_by_name("text")).ok_or(ExitCode::from(1))?
    } else {
        lexer_by_name("text").ok_or(ExitCode::from(1))?
    };

    let lo = lex_options_for(lexer, &args.options).map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(1)
    })?;
    let pre = lexer.preprocess_with(&code, &lo);
    let rendered = if filter_refs.is_empty() {
        format_string(lexer.tokenize(&pre), formatter.as_ref())
    } else {
        let filtered = apply_filters(Some(lexer), lexer.tokenize(&pre), &filter_refs).map_err(|e| {
            eprintln!("pygmentize: {e}");
            ExitCode::from(1)
        })?;
        format_string(filtered.iter().map(|(ty, s)| (*ty, s.as_ref())), formatter.as_ref())
    };
    write_output(args.output.as_deref(), &rendered).map_err(|e| {
        eprintln!("pygmentize: {e}");
        ExitCode::from(1)
    })?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}
