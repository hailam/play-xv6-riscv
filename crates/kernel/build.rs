use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy)]
enum Lang {
    Asm,
    C,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Arch {
    Riscv64,
    Aarch64,
}

fn current_arch() -> Arch {
    match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("riscv64") => Arch::Riscv64,
        Ok("aarch64") => Arch::Aarch64,
        Ok(other) => panic!("unsupported target arch {other}"),
        Err(e) => panic!("CARGO_CFG_TARGET_ARCH unset: {e}"),
    }
}

fn rust_lld_path() -> PathBuf {
    let sysroot = String::from_utf8(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .expect("rustc --print sysroot")
            .stdout,
    )
    .unwrap();
    let host = String::from_utf8(
        Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("rustc -vV")
            .stdout,
    )
    .unwrap();
    let host = host
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .unwrap();
    PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join("gcc-ld")
        .join("ld.lld")
}

fn rust_objcopy_path() -> PathBuf {
    let sysroot = String::from_utf8(
        Command::new("rustc")
            .args(["--print", "sysroot"])
            .output()
            .expect("rustc --print sysroot")
            .stdout,
    )
    .unwrap();
    let host = String::from_utf8(
        Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("rustc -vV")
            .stdout,
    )
    .unwrap();
    let host = host
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .unwrap();
    PathBuf::from(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(host)
        .join("bin")
        .join("llvm-objcopy")
}

fn main() {
    println!("cargo:rerun-if-changed=kernel.ld");
    println!("cargo:rerun-if-changed=kernel-aarch64.ld");
    println!("cargo:rerun-if-changed=user/user.ld");
    println!("cargo:rerun-if-changed=user/user-aarch64.ld");

    let arch = current_arch();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());

    // target/user/<arch>/ for the per-arch ELFs. mkfs reads from
    // here. Riscv keeps the existing target/user/ for backwards
    // compat.
    let target_dir = out_dir.ancestors().nth(5).map(|p| p.to_path_buf());
    let user_dir = target_dir.as_ref().map(|t| match arch {
        Arch::Riscv64 => t.join("user"),
        Arch::Aarch64 => t.join("user-aarch64"),
    });
    if let Some(ud) = user_dir.as_ref() {
        let _ = std::fs::create_dir_all(ud);
    }

    // ---- ulib + umalloc + printf — shared user runtime ----
    // ulib-asm is the arch-specific syscall stubs (ulib.S on riscv,
    // ulib-aarch64.S on aarch64). ulib.c/umalloc.c/printf.c are
    // arch-independent — just C compiled for the target.
    let ulib_asm_obj = compile_to_obj(arch, &out_dir, "ulib", Lang::Asm);
    let ulib_c_obj = compile_to_obj(arch, &out_dir, "ulib", Lang::C);
    let umalloc_obj = compile_to_obj(arch, &out_dir, "umalloc", Lang::C);
    let printf_obj = compile_to_obj(arch, &out_dir, "printf", Lang::C);
    let user_objs: Vec<PathBuf> = vec![ulib_asm_obj, ulib_c_obj, umalloc_obj, printf_obj];

    // initcode is the only user binary we currently emit for
    // aarch64. The rest of the C suite (sh, ls, cat, …) builds
    // riscv-only.
    let initcode_name = match arch {
        Arch::Riscv64 => "initcode",
        Arch::Aarch64 => "initcode-aarch64",
    };
    let initcode_bin =
        build_user_program(arch, &out_dir, initcode_name, Lang::Asm, false, &user_objs);
    println!(
        "cargo:rustc-env=INITCODE_BIN_PATH={}",
        initcode_bin.display()
    );
    if let Some(ud) = user_dir.as_ref() {
        let _ = std::fs::copy(&initcode_bin, ud.join("initcode.elf"));
    }

    // hello.S and pipetest.S are riscv-only assembly demos — skip on
    // aarch64. The rest is C, builds for both arches.
    let programs: &[(&str, &str, Lang, bool, &[Arch])] = &[
        ("echo", "ECHO_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("hello", "HELLO_BIN_PATH", Lang::Asm, false, &[Arch::Riscv64]),
        ("pipetest", "PIPETEST_BIN_PATH", Lang::Asm, false, &[Arch::Riscv64]),
        ("sh", "SH_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("cat", "CAT_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("ls", "LS_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("mkdir", "MKDIR_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("rm", "RM_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("wr", "WR_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("kill", "KILL_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("killtest", "KILLTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("malloctest", "MALLOCTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("smptest", "SMPTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("ln", "LN_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("faulttest", "FAULTTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("xv6test", "XV6TEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("lazytest", "LAZYTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("usertests", "USERTESTS_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("seektest", "SEEKTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("chmodtest", "CHMODTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("credtest", "CREDTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("cloexectest", "CLOEXECTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("trunctest", "TRUNCTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("stattime", "STATTIME_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("sigtest", "SIGTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("sigactest", "SIGACTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("sigmasktest", "SIGMASKTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("fdfiletest", "FDFILETEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("alarmtest", "ALARMTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("ctimetest", "CTIMETEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("envtest", "ENVTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("posix6test", "POSIX6TEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
        ("mmaptest", "MMAPTEST_BIN_PATH", Lang::C, true, &[Arch::Riscv64, Arch::Aarch64]),
    ];
    for (name, env_var, lang, with_ulib, supported_archs) in programs {
        if supported_archs.contains(&arch) {
            let bin =
                build_user_program(arch, &out_dir, name, *lang, *with_ulib, &user_objs);
            println!("cargo:rustc-env={env_var}={}", bin.display());
            if let Some(ud) = user_dir.as_ref() {
                let _ = std::fs::copy(&bin, ud.join(format!("{name}.elf")));
            }
        } else {
            // Satisfy `include_bytes!()` so the kernel still
            // compiles. The bytes will never be referenced because
            // the corresponding embed entry is gated by arch.
            println!("cargo:rustc-env={env_var}={}", initcode_bin.display());
        }
    }
}

fn compile_to_obj(arch: Arch, out_dir: &Path, name: &str, lang: Lang) -> PathBuf {
    // ulib.S is arch-specific. For aarch64 we redirect to ulib-aarch64.S
    // but still emit ulib-S.o so the linker sees a single name. Other
    // assembly stubs (hello.S, pipetest.S, initcode.S) are riscv-only;
    // their aarch64 equivalents have a distinct name.
    let effective_name = match (arch, lang, name) {
        (Arch::Aarch64, Lang::Asm, "ulib") => "ulib-aarch64",
        _ => name,
    };
    let (src, obj_name) = match lang {
        Lang::Asm => (
            format!("user/{effective_name}.S"),
            format!("{name}-S.o"),
        ),
        Lang::C => (
            format!("user/{name}.c"),
            format!("{name}-c.o"),
        ),
    };
    println!("cargo:rerun-if-changed={src}");
    let obj = out_dir.join(obj_name);

    match (arch, lang) {
        (Arch::Riscv64, Lang::Asm) => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64",
            "-nostdlib", "-fno-pie", "-static",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Riscv64, Lang::C) => run("riscv64-elf-gcc", &[
            "-march=rv64gc", "-mabi=lp64", "-mcmodel=medany",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-fno-builtin",
            "-Os", "-Wall",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Aarch64, Lang::Asm) => run("clang", &[
            "--target=aarch64-none-elf", "-march=armv8-a",
            "-mgeneral-regs-only",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
        (Arch::Aarch64, Lang::C) => run("clang", &[
            "--target=aarch64-none-elf", "-march=armv8-a",
            "-mgeneral-regs-only",
            "-nostdlib", "-fno-pie", "-static",
            "-ffreestanding", "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-fno-builtin",
            "-Os", "-Wall",
            "-c", "-o", obj.to_str().unwrap(),
            &src,
        ]),
    }
    obj
}

fn build_user_program(
    arch: Arch,
    out_dir: &Path,
    name: &str,
    lang: Lang,
    with_ulib: bool,
    runtime_objs: &[PathBuf],
) -> PathBuf {
    let obj = compile_to_obj(arch, out_dir, name, lang);
    let elf = out_dir.join(format!("{name}.elf"));
    let stripped = out_dir.join(format!("{name}-stripped.elf"));

    let linker_script = match arch {
        Arch::Riscv64 => "user/user.ld",
        Arch::Aarch64 => "user/user-aarch64.ld",
    };

    let mut ld_args = vec![
        "-T".to_string(),
        linker_script.to_string(),
        "-N".to_string(),
        "-o".to_string(),
        elf.to_str().unwrap().to_string(),
    ];
    if with_ulib {
        for o in runtime_objs {
            ld_args.push(o.to_str().unwrap().to_string());
        }
    }
    ld_args.push(obj.to_str().unwrap().to_string());
    let ld_args_ref: Vec<&str> = ld_args.iter().map(|s| s.as_str()).collect();

    match arch {
        Arch::Riscv64 => run("riscv64-elf-ld", &ld_args_ref),
        Arch::Aarch64 => {
            let lld = rust_lld_path();
            run(lld.to_str().unwrap(), &ld_args_ref)
        }
    }

    // Strip everything that isn't needed at runtime.
    match arch {
        Arch::Riscv64 => run("riscv64-elf-objcopy", &[
            "--strip-all",
            elf.to_str().unwrap(),
            stripped.to_str().unwrap(),
        ]),
        Arch::Aarch64 => {
            let oc = rust_objcopy_path();
            run(oc.to_str().unwrap(), &[
                "--strip-all",
                elf.to_str().unwrap(),
                stripped.to_str().unwrap(),
            ]);
        }
    }

    stripped
}

fn run(prog: &str, args: &[&str]) {
    let status = Command::new(prog)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {prog}: {e}"));
    assert!(status.success(), "{prog} {args:?} exited with {status}");
}
