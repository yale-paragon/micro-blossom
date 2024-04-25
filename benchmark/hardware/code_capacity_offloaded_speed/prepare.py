import os
import sys
import subprocess
from datetime import datetime
from run import *
from build_micro_blossom import main as build_micro_blossom_main
from get_ttyoutput import get_ttyoutput


def main():
    compile_code_if_necessary()

    if not os.path.exists(hardware_dir):
        os.mkdir(hardware_dir)

    p = min(p_vec)  # use the minimum p to build the hardware
    for d in d_vec:
        # first generate the graph config file
        syndrome_file_path = os.path.join(hardware_dir, f"d_{d}.syndromes")
        if not os.path.exists(syndrome_file_path):
            command = fusion_blossom_qecp_generate_command(
                d=d, p=p, total_rounds=10, noisy_measurements=0
            )
            command += ["--code-type", "rotated-planar-code"]
            command += [
                "--decoder",
                "fusion",
                "--decoder-config",
                '{"only_stab_z":true,"use_combined_probability":true,"skip_decoding":true,"max_half_weight":1}',
            ]
            command += [
                "--debug-print",
                "fusion-blossom-syndrome-file",
                "--fusion-blossom-syndrome-export-filename",
                syndrome_file_path,
            ]
            print(command)
            stdout, returncode = run_command_get_stdout(command)
            print("\n" + stdout)
            assert returncode == 0, "command fails..."

        # then generate the graph json
        graph_file_path = os.path.join(hardware_dir, f"d_{d}.json")
        if not os.path.exists(graph_file_path):
            command = micro_blossom_command() + ["parser"]
            command += [syndrome_file_path]
            command += ["--graph-file", graph_file_path]
            print(command)
            stdout, returncode = run_command_get_stdout(command)
            print("\n" + stdout)
            assert returncode == 0, "command fails..."

        # create the hardware project
        if not os.path.exists(hardware_proj_dir(d)):
            parameters = ["--name", hardware_proj_name(d)]
            parameters += ["--path", hardware_dir]
            parameters += ["--clock-frequency", "100"]
            parameters += ["--graph", graph_file_path]
            build_micro_blossom_main(parameters)

    # then build hello world application
    make_env = os.environ.copy()
    make_env["EMBEDDED_BLOSSOM_MAIN"] = "hello_world"
    process = subprocess.Popen(
        ["make", "Xilinx"],
        universal_newlines=True,
        stdout=sys.stdout,
        stderr=sys.stderr,
        cwd=embedded_dir,
        env=make_env,
    )
    process.wait()
    assert process.returncode == 0, "compile error"

    # build all hardware projects using the hello world application
    for d in d_vec:
        log_file_path = os.path.join(hardware_proj_dir(d), "build.log")
        print(f"building d={d}, log output to {log_file_path}")
        if not os.path.exists(
            os.path.join(hardware_proj_dir(d), f"{hardware_proj_name(d)}.xsa")
        ):
            with open(log_file_path, "a") as log:
                process = subprocess.Popen(
                    ["make"],
                    universal_newlines=True,
                    stdout=log.fileno(),
                    stderr=log.fileno(),
                    cwd=hardware_proj_dir(d),
                )
                process.wait()
                assert process.returncode == 0, "synthesis error"

    # run the hello world application and run on hardware for sanity check
    for d in d_vec:
        log_file_path = os.path.join(hardware_proj_dir(d), "make.log")
        print(f"testing d={d}, log output to {log_file_path}")
        with open(log_file_path, "a", encoding="utf8") as log:
            log.write(
                f"[host_event] [make run_a72 start] {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n"
            )
            tty_output, command_output = get_ttyoutput(
                command=["make", "run_a72"], cwd=hardware_proj_dir(d), silent=True
            )
            log.write(
                f"[host_event] [make run_a72 finish] {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n"
            )
            log.write(f"[host_event] [tty_output]\n")
            log.write(tty_output + "\n")
            log.write(f"[host_event] [command_output]\n")
            log.write(command_output + "\n")
            assert "Hello world!" in tty_output


if __name__ == "__main__":
    main()
