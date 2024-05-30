import os, sys, git
from dataclasses import dataclass

git_root_dir = git.Repo(".", search_parent_directories=True).working_tree_dir
sys.path.insert(0, os.path.join(git_root_dir, "benchmark"))
sys.path.insert(0, os.path.join(git_root_dir, "src", "fpga", "utils"))
from vivado_builder import *
from frequency_explorer import *


@dataclass
class Configuration:
    inject_registers: list[str]
    broadcast_delay: int = 0

    frequency: float = 250  # Axi4 bus
    clock_divide_by: float = 1  # start with 250MHz slow clock

    def name(self) -> str:
        return f"registers_{'_'.join(self.inject_registers)}_b{self.broadcast_delay}"


this_dir = os.path.dirname(os.path.abspath(__file__))
frequency_log_dir = os.path.join(this_dir, "frequency_log")
if not os.path.exists(frequency_log_dir):
    os.mkdir(frequency_log_dir)


graph_builder = MicroBlossomGraphBuilder(
    graph_folder=os.path.join(this_dir, "tmp-graph"),
    name="d_9_circuit_level_full",
    d=9,
    p=0.001,
    noisy_measurements=9 - 1,
    max_half_weight=7,
    visualize_graph=True,
)

configurations = [
    # Configuration(inject_registers=[]),
    # Configuration(inject_registers=[], broadcast_delay=1),
    # Configuration(inject_registers=["execute"]),
    # Configuration(inject_registers=["execute"], broadcast_delay=1),
    # Configuration(inject_registers=["execute,update"]),
    Configuration(inject_registers=["execute,update"], broadcast_delay=1),
]


def get_project(
    configuration: Configuration, slow_frequency: int
) -> MicroBlossomAxi4Builder:
    return MicroBlossomAxi4Builder(
        graph_builder=graph_builder,
        name=configuration.name() + f"_sf{slow_frequency}",
        clock_frequency=configuration.frequency,
        clock_divide_by=configuration.frequency / slow_frequency,
        project_folder=os.path.join(this_dir, "tmp-project"),
        broadcast_delay=configuration.broadcast_delay,
        inject_registers=configuration.inject_registers,
    )


results = ["# <context depth> <best frequency/MHz>"]
for configuration in configurations:

    def compute_next_maximum_slow_frequency(slow_frequency: int) -> int:
        project = get_project(configuration, slow_frequency)
        project.build()
        new_clock_divide_by = project.next_minimum_clock_divide_by()
        return project.clock_frequency / new_clock_divide_by

    explorer = FrequencyExplorer(
        compute_next_maximum_frequency=compute_next_maximum_slow_frequency,
        log_filepath=os.path.join(frequency_log_dir, configuration.name() + ".txt"),
    )

    best_slow_frequency = explorer.optimize()
    print(f"{configuration.name()}: {best_slow_frequency}MHz")
    results.append(f"{configuration.context_depth} {best_slow_frequency}")

    # project = get_project(configuration, best_frequency)

with open("best_slow_frequencies.txt", "w", encoding="utf8") as f:
    f.write("\n".join(results))
