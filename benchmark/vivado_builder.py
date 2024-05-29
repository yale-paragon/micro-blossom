import os, sys, git, subprocess, math
from dataclasses import dataclass, field

git_root_dir = git.Repo(".", search_parent_directories=True).working_tree_dir
sys.path.insert(0, os.path.join(git_root_dir, "benchmark"))
sys.path.insert(0, os.path.join(git_root_dir, "src", "fpga", "utils"))
from micro_util import *
from build_micro_blossom import main as build_micro_blossom_main
from vivado_project import VivadoProject


@dataclass
class MicroBlossomGraphBuilder:
    """build the graph using QEC-Playground"""

    graph_folder: str
    name: str
    d: int
    p: float
    noisy_measurements: int
    max_half_weight: int
    code_type: str = "rotated-planar-code"
    noise_model: str = "stim-noise-model"
    only_stab_z: bool = True
    use_combined_probability: bool = True
    test_syndrome_count: int = 100
    transform_graph: bool = True
    visualize_graph: bool = False

    def decoder_config(self):
        return {
            "only_stab_z": self.only_stab_z,
            "use_combined_probability": self.use_combined_probability,
            "skip_decoding": True,
            "max_half_weight": self.max_half_weight,
        }

    def graph_file_path(self) -> str:
        return os.path.join(self.graph_folder, f"{self.name}.json")

    def syndrome_file_path(self) -> str:
        return os.path.join(self.graph_folder, f"{self.name}.syndromes")

    def build(self) -> None:
        if not os.path.exists(self.graph_folder):
            os.mkdir(self.graph_folder)

        # first create the syndrome file
        syndrome_file_path = self.syndrome_file_path()
        if not os.path.exists(syndrome_file_path):
            command = fusion_blossom_qecp_generate_command(
                d=self.d,
                p=self.p,
                total_rounds=self.test_syndrome_count,
                noisy_measurements=self.noisy_measurements,
            )
            command += ["--code-type", self.code_type]
            command += ["--noise-model", self.noise_model]
            command += [
                "--decoder",
                "fusion",
                "--decoder-config",
                json.dumps(self.decoder_config(), separators=(",", ":")),
            ]
            command += [
                "--debug-print",
                "fusion-blossom-syndrome-file",
                "--fusion-blossom-syndrome-export-filename",
                syndrome_file_path,
            ]
            command += ["--parallel", f"0"]  # use all cores
            print(command)
            stdout, returncode = run_command_get_stdout(command)
            print("\n" + stdout)
            assert returncode == 0, "command fails..."

            # merge two side of the virtual vertices to reduce resource usage
            if self.transform_graph:
                if self.code_type == "rotated-planar-code":
                    command = micro_blossom_command() + [
                        "transform-syndromes",
                        syndrome_file_path,
                        syndrome_file_path,
                        "qecp-rotated-planar-code",
                        f"{self.d}",
                    ]
                    stdout, returncode = run_command_get_stdout(command)
                    print("\n" + stdout)
                    assert returncode == 0, "command fails..."
                else:
                    raise Exception(f"transform not implemented for ${self.code_type}")

            if self.visualize_graph:
                command = fusion_blossom_command() + [
                    "visualize-syndromes",
                    syndrome_file_path,
                    "--visualizer-filename",
                    f"micro_blossom_{self.name}.json",
                ]
                stdout, returncode = run_command_get_stdout(command)
                print("\n" + stdout)
                assert returncode == 0, "command fails..."

        # then generate the graph json
        graph_file_path = self.graph_file_path()
        if not os.path.exists(graph_file_path):
            command = micro_blossom_command() + ["parser"]
            command += [syndrome_file_path]
            command += ["--graph-file", graph_file_path]
            print(command)
            stdout, returncode = run_command_get_stdout(command)
            print("\n" + stdout)
            assert returncode == 0, "command fails..."


@dataclass
class MicroBlossomAxi4Builder:
    graph_builder: MicroBlossomGraphBuilder

    project_folder: str
    name: str
    clock_frequency: float = 200  # in MHz
    clock_divide_by: int = 2
    # e.g. ["offload"], ["offload", "update3"]
    inject_registers: list[str] = field(default_factory=lambda: [])
    overwrite: bool = False

    def hardware_proj_dir(self) -> str:
        return os.path.join(self.project_folder, self.name)

    def prepare_graph(self):
        self.graph_builder.build()

    def create_vivado_project(self):
        if not os.path.exists(self.project_folder):
            os.mkdir(self.project_folder)
        if not os.path.exists(self.hardware_proj_dir()) or not os.path.exists(
            os.path.join(
                self.hardware_proj_dir(), f"{self.name}_verilog", "MicroBlossomBus.v"
            )
        ):
            parameters = ["--name", self.name]
            parameters += ["--path", self.project_folder]
            parameters += ["--clock-frequency", f"{self.clock_frequency}"]
            parameters += ["--clock-divide-by", f"{self.clock_divide_by}"]
            parameters += ["--graph", self.graph_builder.graph_file_path()]
            parameters += ["--inject-registers"] + self.inject_registers
            if self.overwrite:
                parameters += ["--overwrite"]
            build_micro_blossom_main(parameters)

    def build_vivado_project(self):
        log_file_path = os.path.join(self.hardware_proj_dir(), "build.log")
        frequency = self.clock_frequency
        print(f"building frequency={frequency}, log output to {log_file_path}")
        xsa_path = os.path.join(self.hardware_proj_dir(), f"{self.name}.xsa")
        if not os.path.exists(xsa_path):
            # TODO: build embedded binary first
            with open(log_file_path, "a") as log:
                process = subprocess.Popen(
                    ["make"],
                    universal_newlines=True,
                    stdout=log.fileno(),
                    stderr=log.fileno(),
                    cwd=self.hardware_proj_dir(),
                )
                process.wait()
                assert process.returncode == 0, "synthesis error"

    # return current frequency if timing passed; otherwise return a maximum frequency that is achievable
    def next_maximum_frequency(self) -> int:
        vivado = VivadoProject(self.hardware_proj_dir())
        wns = vivado.routed_timing_summery().clk_pl_0_wns
        frequency = vivado.frequency()
        if wns < 0:
            print(f"frequency={frequency}MHz clock frequency too high!!!")
            period = 1e-6 / frequency
            new_period = period - wns * 1e-9
            new_frequency = math.floor(1 / new_period / 1e6)
            print(f"wns: {wns}ns, should lower the frequency to {new_frequency}MHz")
            return new_frequency
        else:
            return frequency

    def build(self):
        self.prepare_graph()
        self.create_vivado_project()
        self.build_vivado_project()
