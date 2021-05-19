use parking_lot::Mutex;
use clap::{App, Arg, value_t};
use godwoken_test::specs::{SimpleCase};
use godwoken_test::{worker::{Notify, Workers}, Spec};
use std::any::Any;
use std::env;
// use std::io::{self, Write};
use std::time::{Duration, Instant};
use crossbeam_channel::{unbounded};
use std::sync::Arc;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
enum TestResultStatus {
    Passed,
    Failed,
    Panicked,
}

struct TestResult {
  spec_name: String,
  status: TestResultStatus,
  duration: u64,
}

// #[allow(clippy::cognitive_complexity)]
fn main() {
  env::set_var("RUST_BACKTRACE", "full");

  let clap_app = clap_app();
  let matches = clap_app.get_matches();

  let max_time = if matches.is_present("max-time") {
    value_t!(matches, "max-time", u64).unwrap_or_else(|err| err.exit())
  } else { 0 };
  let spec_names_to_run: Vec<_> = matches.values_of("specs").unwrap_or_default().collect();
  let verbose = matches.is_present("verbose");

  // You can check the value provided by positional arguments, or option arguments
  if let Some(o) = matches.value_of("output") {
      println!("Value for output: {}", o);
  }

  if let Some(c) = matches.value_of("config") {
      println!("Value for config: {}", c);
  }

  // You can see how many times a particular flag or argument occurred
  // Note, only flags can have multiple occurrences
  match matches.occurrences_of("debug") {
      0 => println!("Debug mode is off"),
      1 => println!("Debug mode is kind of on"),
      2 => println!("Debug mode is on"),
      _ => println!("Don't be crazy"),
  }

  // You can check for the existence of subcommands, and if found use their
  // matches just as you would the top level app
  if let Some(ref matches) = matches.subcommand_matches("test") {
      // "$ myapp test" was run
      if matches.is_present("list") {
          // "$ myapp test -l" was run
          println!("Printing testing lists...");
      } else {
          println!("Not printing testing lists...");
      }
  }

  let start_time = Instant::now();
  let specs = filter_specs(all_specs(), spec_names_to_run);
  let total = specs.len();
  let worker_count = value_t!(matches, "concurrent", usize).unwrap_or_else(|err| err.exit());
  let specs = Arc::new(Mutex::new(specs));
  let mut spec_errors: Vec<Option<Box<dyn Any + Send>>> = Vec::new();
  let mut error_spec_names = Vec::new();
  let mut test_results = Vec::new();
  // start x workers
  let (notify_tx, notify_rx) = unbounded();

  log::info!("start {} workers...", worker_count);
  let mut workers = Workers::new(worker_count, Arc::clone(&specs), notify_tx, 9999);
  workers.start();

  let mut worker_running = worker_count;
  let mut done_specs = 0;
  while worker_running > 0 {
    if max_time > 0 && start_time.elapsed().as_secs() > max_time {
      workers.shutdown();
    }
    let msg = match notify_rx.recv_timeout(Duration::from_secs(5)) {
      Ok(msg) => msg,
      Err(err) => {
        if err.is_timeout() {
          continue;
        }
        panic!(err);
      }
    };
    match msg {
      Notify::Start { spec_name } => {
        log::info!("[{}] Start executing", spec_name);
      }
      Notify::Error {
        spec_error,
        spec_name,
        seconds,
      } => {
          test_results.push(TestResult {
            spec_name: spec_name.clone(),
            status: TestResultStatus::Failed,
            duration: seconds,
          });
          error_spec_names.push(spec_name.clone());
          // rerun_specs.push(spec_name.clone());
          // if fail_fast {
          //   workers.shutdown();
          //   worker_running -= 1;
          // }
          spec_errors.push(Some(spec_error));
          if verbose {
            log::info!("[{}] Error", spec_name);
            // tail_node_logs(&node_log_paths);
          }
      }
      Notify::Panick {
        spec_name,
        seconds,
        // node_log_paths,
      } => {
        test_results.push(TestResult {
          spec_name: spec_name.clone(),
          status: TestResultStatus::Panicked,
          duration: seconds,
        });
        error_spec_names.push(spec_name.clone());
        // rerun_specs.push(spec_name.clone());
        // if fail_fast {
        //   workers.shutdown();
        //   worker_running -= 1;
        // }
        spec_errors.push(None);
        if verbose {
          log::info!("[{}] Panic", spec_name);
            // print_panicked_logs(&node_log_paths);
        }
      }
      Notify::Done {
        spec_name,
        seconds,
        // node_paths,
      } => {
        test_results.push(TestResult {
          spec_name: spec_name.clone(),
          status: TestResultStatus::Passed,
          duration: seconds,
        });
        done_specs += 1;
        log::info!(
          "{}/{} .............. [{}] Done in {} seconds",
          done_specs, total, spec_name, seconds
        );
      }
      Notify::Stop => {
        worker_running -= 1;
      }
    }
  }

  test_results.push(TestResult {
    spec_name: "other test case could be added into [tests/src/specs/]".to_string(),
    status: TestResultStatus::Failed,
    duration: 999,
  });
  print_results(test_results);
  println!("Total elapsed time: {:?}", start_time.elapsed());
}

/// all test cases
fn all_specs() -> Vec<Box<dyn Spec>> {
  vec![
    Box::new(SimpleCase)
  ]
  //TODO: shuffle
}

fn filter_specs(
  mut all_specs: Vec<Box<dyn Spec>>,
  spec_names_to_run: Vec<&str>,
) -> Vec<Box<dyn Spec>> {
  if spec_names_to_run.is_empty() {
    return all_specs;
  }

  for name in spec_names_to_run.iter() {
    if !all_specs.iter().any(|spec| spec.name() == *name) {
      eprintln!("Unknown spec {}", name);
      std::process::exit(1);
    }
  }

  all_specs.retain(|spec| spec_names_to_run.contains(&spec.name()));
  all_specs
}

/// create a new instance of clap app
// TODO:
//    - A config file
//        + Uses "-c filename" or "--config filename"
//    - An output file
//        + A positional argument (i.e. "$ myapp output_filename")
//    - A help flag (automatically generated by clap)
//        + Uses "-h" or "--help" (Only autogenerated if you do NOT specify your own "-h" or "--help")
//    - A version flag (automatically generated by clap)
//        + Uses "-V" or "--version" (Only autogenerated if you do NOT specify your own "-V" or "--version")
fn clap_app() -> App<'static, 'static> {
  App::new("ckb-test").version("0.0.2")
      .arg(
        Arg::with_name("max-time")
            .long("max-time")
            .takes_value(true)
            .value_name("SECONDS")
            .help("Exit when total running time exceeds this limit"),
      )
      // .arg(Arg::with_name("list-specs").long("list-specs"))
      .arg(Arg::with_name("specs").multiple(true))
      .arg(
        Arg::with_name("concurrent")
            .short("c")
            .long("concurrent")
            .takes_value(true)
            .help("The number of specs can running concurrently")
            .default_value("1"),
      )
      .arg(
        Arg::with_name("verbose")
            .long("verbose")
            .help("Show verbose log"),
      )
      .arg(
          Arg::with_name("no-report")
              .long("no-report")
              .help("Do not show integration test report"),
      )
      .arg(
          Arg::with_name("log-file")
              .long("log-file")
              .takes_value(true)
              .help("Write log outputs into file."),
      )

      // .about("Does awesome things")
      // .arg(
      // 		Arg::with_name("binary")
      // 				.short("b")
      // 				.long("bin")
      // 				.takes_value(true)
      // 				.value_name("PATH")
      // 				.help("Path to ckb executable")
      // 				.default_value("../target/release/godwoken"),
      // )
      // .get_matches();
      // .arg(
      // 		Arg::new("config")
      // 				.short('c')
      // 				.long("config")
      // 				.value_name("FILE")
      // 				.about("Sets a custom config file")
      // 				.takes_value(true),
      // )
      // .arg(
      // 		Arg::new("output")
      // 				.about("Sets an optional output file")
      // 				.index(1),
      // )
}


fn print_results(mut test_results: Vec<TestResult>) {
  println!("{}", "-".repeat(80));
  println!("{:600} | {:10} | {:10}", "TEST", "STATUS", "DURATION");

  test_results.sort_by(|a, b| a.status.cmp(&b.status));

  for result in test_results.iter() {
    println!(
      "{:60} | {:10} | {:<10}",
      result.spec_name,
      format!("{:?}", result.status),
      format!("{} s", result.duration),
    );
  }
}
