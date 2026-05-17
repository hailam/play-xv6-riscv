use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy)]
enum Lang {
    Asm,
    C,
}

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    println!("cargo:rerun-if-changed=user/user.ld");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    // Resolve workspace `target/` from OUT_DIR. OUT_DIR is
    // `<target>/<triple>/<profile>/build/<crate>-<hash>/out`, so 5
    // ancestors up == target/. Copies stripped user ELFs there with
    // stable filenames so mkfs can find them.
    let target_dir = out_dir.ancestors().nth(5).map(|p| p.to_path_buf());
    let user_dir = target_dir.as_ref().map(|t| t.join("user"));
    if let Some(ud) = user_dir.as_ref() {
        let _ = std::fs::create_dir_all(ud);
    }

    let ulib_obj = compile_to_obj(&out_dir, "ulib", Lang::Asm);

    // Only `initcode` is `include_bytes!`'d into the kernel; every
    // other binary now lives on disk and is loaded via `sys_exec`.
    // We still build them all here because mkfs reads them out of
    // `target/user/`.
    let programs: &[(&str, &str, Lang, bool)] = &[
        ("initcode", "INITCODE_BIN_PATH", Lang::Asm, false),
        ("echo", "ECHO_BIN_PATH", Lang::C, true),
        ("hello", "HELLO_BIN_PATH", Lang::Asm, false),
        ("pipetest", "PIPETEST_BIN_PATH", Lang::Asm, false),
        ("sh", "SH_BIN_PATH", Lang::C, true),
        ("cat", "CAT_BIN_PATH", Lang::C, true),
        ("ls", "LS_BIN_PATH", Lang::C, true),
        ("mkdir", "MKDIR_BIN_PATH", Lang::C, true),
        ("rm", "RM_BIN_PATH", Lang::C, true),
        ("wr", "WR_BIN_PATH", Lang::C, true),
        ("kill", "KILL_BIN_PATH", Lang::C, true),
        ("killtest", "KILLTEST_BIN_PATH", Lang::C, true),
    ];

    for (name, env_var, lang, with_ulib) in programs {
        let bin = build_user_program(&out_dir, name, *lang, *with_ulib, &ulib_obj);
        println!("cargo:rustc-env={env_var}={}", bin.display());
        if let Some(ud) = user_dir.as_ref() {
            let stable = ud.join(format!("{name}.elf"));
            let _ = std::fs::copy(&bin, &stable);
        }
    }
}

fn compile_to_obj(out_dir: &Path, name: &str, lang: Lang) -> PathBuf {
    let src = match lang {
        Lang::Asm => format!("user/{name}.S"),
        Lang::C => format!("user/{name}.c"),
    };
    println!("cargo:rerun-if-changed={src}");
    let obj = out_dir.join(format!("{name}.o"));
    match lang {
        Lang::Asm => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64",
            "-nostdlib", "-fno-pie", "-static",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        Lang::C => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64", "-mcmodel=medany",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-Os", "-Wall",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
    }
    obj
}

fn build_user_program(
    out_dir: &Path,
    name: &str,
    lang: Lang,
    with_ulib: bool,
    ulib_obj: &Path,
) -> PathBuf {
    let obj = compile_to_obj(out_dir, name, lang);
    let elf = out_dir.join(format!("{name}.elf"));
    let stripped = out_dir.join(format!("{name}-stripped.elf"));

    let mut ld_args = vec![
        "-T".to_string(),
        "user/user.ld".to_string(),
        "-N".to_string(),
        "-o".to_string(),
        elf.to_str().unwrap().to_string(),
    ];
    if with_ulib {
        ld_args.push(ulib_obj.to_str().unwrap().to_string());
    }
    ld_args.push(obj.to_str().unwrap().to_string());
    let ld_args_ref: Vec<&str> = ld_args.iter().map(|s| s.as_str()).collect();
    run("riscv64-elf-ld", &ld_args_ref);

    // Strip everything that isn't needed at runtime; the kernel ELF
    // loader only reads the program header table and segment data.
    run("riscv64-elf-objcopy", &[
        "--strip-all",
        elf.to_str().unwrap(),
        stripped.to_str().unwrap(),
    ]);

    stripped
}

fn run(prog: &str, args: &[&str]) {
    let status = Command::new(prog)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {prog}: {e}"));
    assert!(status.success(), "{prog} {args:?} exited with {status}");
}
