import os
from common import *
import vitis # see <Vitis_Installation_Dir>/cli/examples for examples

# open or create workspace
if os.path.exists(workspace):
    client = vitis.create_client()
    client.set_workspace(workspace)
else:
    client = vitis.create_client(workspace=workspace)

# create platform from XSA only if not exists
if not client.list_platforms():
    platform = client.create_platform_component(name=name, hw=f"./{name}.xsa", os="standalone", cpu=cpus[0], domain_name=f"standalone_{cpus[0]}")
    for cpu in cpus[1:]:
        platform.add_domain(cpu=cpu, os="standalone", name=f"standalone_{cpu}")
    status = platform.build()
    # print(status)
    # print(platform.list_domains())
    platform.report()

# create application component only if not exists
platform_xpfm = client.get_platform(name)
for cpu_id, cpu, arch in zip(cpu_ids, cpus, archs):
    try:
        component = client.get_component(name=f"benchmark_{cpu_id}")
    except Exception:
        component = client.create_app_component(name=f"benchmark_{cpu_id}", platform=platform_xpfm, domain=f"standalone_{cpu}")
    # import source file and patch the application
    component.import_files(from_loc="./src", files=import_files, dest_dir_in_cmp="src")
    rust_staticlib = os.path.join(os.path.abspath(rust_project), "target", arch, profile, f"{rust_libname}.a")
    assert os.path.exists(rust_staticlib), f"rust static lib not found at {rust_staticlib}, please compile it"
    component.set_app_config(key="USER_LINK_LIBRARIES", values=rust_staticlib)
    component.set_app_config(key="USER_COMPILE_OPTIMIZATION_OTHER_FLAGS", values="-flto")  # enable link-time optimization
    component.set_app_config(key="USER_LINK_OTHER_FLAGS", values="-Wl,-gc-sections")  # remove unused function sections
    ld_script = component.get_ld_script()
    if cpu_id == "r5":
        # avoid going through DRAM for stack and statically allocated objects
        # use lock-step mode (disable split mode) so that all 256KB TCM is available to RPU core 0
        ld_script.update_memory_region(name="psv_r5_tcm_ram_0", base_address="0", size="0x40000")  # 256KB
        for section in [".stack", ".bss", ".sbss", ".tbss"]:
            ld_script.update_ld_section(section=section, region="psv_r5_tcm_ram_0")
        # also put program into OCM
        ld_script.update_ld_section(section=".text", region="psv_ocm_0")
        ld_script.update_ld_section(section=".data", region="psv_ocm_0")
    # component.clean()  # clean build: it doesn't take too long anyway, just linking the Rust program
    component.build(target="hw")
