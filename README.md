# Distributed MWPM Decoder System

Distributed MWPM decoder for Quantum Error Correction

## Project Structure

- src: source code
  - fpga: the FPGA source code, including generator scripts
  - cpu: the CPU source code
- projects: Vivado projects


## Installation

For FPGA development and testing, we use [Verilator](https://verilator.org/guide/latest/install.html).
We pin to a specific version 5.014 to avoid incompatibility.

```sh
sudo apt install gtkwave
sudo apt install git help2man perl python3 make autoconf g++ flex bison ccache
sudo apt install libgoogle-perftools-dev numactl perl-doc
sudo apt-get install libfl2  # Ubuntu only (ignore if gives error)
sudo apt-get install libfl-dev  # Ubuntu only (ignore if gives error)
sudo apt-get install zlibc zlib1g zlib1g-dev  # Ubuntu only (ignore if gives error)

git clone https://github.com/verilator/verilator   # Only first time
cd verilator
git pull
git checkout v5.014      # pin to this specific version 

autoconf         # Create ./configure script
./configure      # Configure and create Makefile
make -j `nproc`  # Build Verilator itself (if error, try just 'make')
sudo make install
```
