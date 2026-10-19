//! Standalone subprocess fixture, compiled by the dispatcher integration tests.
use std::{
    fs,
    io::{self, Read},
    path::Path,
    process::{self, Command},
    thread,
    time::{Duration, Instant},
};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = &args[1];
    match mode.as_str() {
        "check-exists" => {
            assert!(Path::new(&args[2]).exists());
            return;
        }
        "check-fail" => process::exit(4),
        "check-sleep" => {
            thread::sleep(Duration::from_millis(args[2].parse().unwrap()));
            return;
        }
        "check-child" => {
            let child = Command::new("sleep").arg("30").spawn().unwrap();
            fs::write(&args[2], child.id().to_string()).unwrap();
            return;
        }
        _ => {}
    }

    let ticket = &args[2];
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt).unwrap();
    assert!(prompt.contains(&format!("Implement only ticket {ticket}:")));
    assert!(prompt.contains("Owned paths:"));
    assert!(prompt.contains("Deliver "));
    match mode.as_str() {
        "need-a" => assert_eq!(fs::read_to_string("a.txt").unwrap(), "a\n"),
        "barrier" => {
            let directory = Path::new(&args[3]);
            fs::write(directory.join(ticket), "ready").unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            while !(directory.join("a").exists() && directory.join("b").exists()) {
                assert!(
                    Instant::now() < deadline,
                    "workers did not run concurrently"
                );
                thread::sleep(Duration::from_millis(20));
            }
        }
        "slow" => thread::sleep(Duration::from_secs(30)),
        "child-fail" if !Path::new(&args[4]).exists() => {
            let child = Command::new("sleep").arg("30").spawn().unwrap();
            fs::write(&args[3], child.id().to_string()).unwrap();
            process::exit(7);
        }
        "fail" if !Path::new(&args[3]).exists() => process::exit(7),
        "outside" => fs::write("outside.txt", "bad").unwrap(),
        "main-dirty" => fs::write(&args[3], "unrelated edit").unwrap(),
        "git-rm" => {
            assert!(
                Command::new("git")
                    .args(["rm", "-q", "README.md"])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        "branch" => {
            assert!(
                Command::new("git")
                    .args(["switch", "-c", "other-worker-branch"])
                    .status()
                    .unwrap()
                    .success()
            );
        }
        _ => {}
    }

    if mode != "empty" {
        fs::write(format!("{ticket}.txt"), format!("{ticket}\n")).unwrap();
    }
    if mode == "commit" {
        assert!(
            Command::new("git")
                .args(["add", "--all"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "forbidden"])
                .status()
                .unwrap()
                .success()
        );
    }
}
