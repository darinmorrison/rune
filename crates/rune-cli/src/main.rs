use anyhow::{bail, Result};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

use rune::SpannedError as _;

fn compile(source: &str) -> rune::Result<st::Unit> {
    let unit = rune::parse_all::<rune::ast::File>(&source)?;
    Ok(unit.encode()?)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let mut args = env::args();
    args.next();

    let mut path = None;
    let mut trace = false;
    let mut dump_unit = false;
    let mut dump_vm_state = false;
    let mut help = false;

    for arg in args {
        match arg.as_str() {
            "--trace" => {
                trace = true;
            }
            "--dump" => {
                dump_unit = true;
                dump_vm_state = true;
            }
            "--dump-unit" => {
                dump_unit = true;
            }
            "--dump-vm-state" => {
                dump_vm_state = true;
            }
            "--help" => {
                help = true;
            }
            other => {
                path = Some(PathBuf::from(other));
            }
        }
    }

    const USAGE: &str = "rune-cli [--trace] [--dump-unit] [--dump-vm-state] <file>";

    if help {
        println!("Usage: {}", USAGE);
        println!();
        println!("  --trace         - Provide detailed tracing for each instruction executed.");
        println!("  --dump          - Dump all forms of diagnostic.");
        println!("  --dump-unit     - Dump diagnostics on the unit generated from the file.");
        println!("  --dump-vm-state - Dump diagnostics on VM state (stack).");
        return Ok(());
    }

    let path = match path {
        Some(path) => PathBuf::from(path),
        None => {
            bail!("Invalid usage: {}", USAGE);
        }
    };

    let source = fs::read_to_string(&path)?;

    let unit = match compile(&source) {
        Ok(unit) => unit,
        Err(e) => {
            let span = e.span();
            let thing = &source[span.start..span.end];
            let (line, col) = span.line_col(&source);

            println!(
                "{} at {}:{}:{} {}",
                e,
                path.display(),
                line + 1,
                col + 1,
                thing
            );

            let mut i = 0;
            let mut e = &e as &dyn Error;

            while let Some(err) = e.source() {
                println!("#{}: {}", i, err);
                i += 1;
                e = err;
            }

            return Ok(());
        }
    };

    if dump_unit {
        println!("# unit dump");
        println!("instructions:");

        for (i, inst) in unit.iter_instructions().enumerate() {
            println!("{:04x} = {:?}", i, inst);
        }

        println!("functions:");

        for (hash, f) in unit.iter_functions() {
            println!("{} = {:?}", hash, f);
        }

        println!("strings:");

        for (hash, string) in unit.iter_static_strings() {
            println!("{} = {:?}", hash, string);
        }

        println!("---");
    }

    let mut vm = st::Vm::new();
    let functions = st::Functions::with_default_packages()?;

    let mut task: st::Task<st::Value> = vm.call_function(&functions, &unit, "main", ())?;

    let last = std::time::Instant::now();

    let result = loop {
        if trace {
            println!(
                "ip:{:04x} = {:?}",
                task.ip,
                task.unit.instruction_at(task.ip)
            );
        }

        let result = task.step().await;

        let result = match result {
            Ok(result) => result,
            Err(e) => {
                println!("#0: {}", e);
                let mut e = &e as &dyn Error;
                let mut i = 1;

                while let Some(err) = e.source() {
                    println!("#{}: {}", i, err);
                    i += 1;
                    e = err;
                }

                return Ok(());
            }
        };

        if trace && dump_vm_state {
            println!("# stack dump");

            for (n, (slot, value)) in task.vm.iter_stack_debug().enumerate() {
                println!("{} = {:?} ({:?})", n, slot, value);
            }

            println!("---");
        }

        if let Some(result) = result {
            break result;
        }
    };

    let duration = std::time::Instant::now().duration_since(last);
    println!("== {:?} ({:?})", result, duration);

    if dump_vm_state {
        println!("# stack dump after completion");

        for (n, (slot, value)) in vm.iter_stack_debug().enumerate() {
            println!("{} = {:?} ({:?})", n, slot, value);
        }

        println!("---");
    }

    Ok(())
}