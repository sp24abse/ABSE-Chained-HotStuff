# ABSE-Chained-HotStuff

## Running in release mode (the performance in release mode is more than 10 times higher than in debug mode)

```Bash
cargo build --release
```
Find ach under target/release, run
```Bash
./ach -h
```
as well as
```Bash
./ach config-gen --help
```
to view parameter definitions.

The subcommand `config-gen` provide a way to generate multiple files for multi replicas over multi hosts.
It also helps to generate a bunch of bash scripts to distribute files in accordance, run all the nodes, and
collect the results.

Please lookup the document of config-gen before using it.

**Remember that, default `config-gen` will run in dry-run mode, in which all the files will be print to the console.
By specify `-w` you can flush these files to disks.**

Example:

Create a new localhost folder in the parent directory and generate relevant configuration files (injection rate: 130, batch size: 50, transaction size: 128, max jump: 6, timeout: 1000, 16 processes, 5 fauties):

```Bash
./ach -r 130 -b 50 -t 128 -m 6 --timeout 1000 config-gen -n 16 -f 5 -e ../ -w localhost
```

 Then copy ach to the localhost directory and run 
 ```Bash
 bash run.sh 
```
(View more specific logs, including the process of ABSE, by adding 'RUST_LOG=TRACE' to the process)

 Currently, it is not supported to adjust the baseline parameters after compilation. Please modify the baseline logic in the abse.rs and recompile the system.

## Quick Start

Directly executing a binary will start a committee composed of a single node using localhost TCP for communication.

```Bash
cargo r
```

## Config Generation

The subcommand `config-gen` provide a way to generate multiple files for multi replicas over multi hosts.
It also helps to generate a bunch of bash scripts to distribute files in accordance, run all the nodes, and
collect the results.

Please lookup the document of config-gen before using it.

`cargo r -- config-gen --help`

**Remember that, default `config-gen` will run in dry-run mode, in which all the files will be print to the console.
By specify `-w` you can flush these files to disks.**

Let's say we want to distribute 4 replicas over 2 hosts (IP_OF_SERVER_1, IP_OF_SERVER_2).

```Bash
cargo r -- config-gen --number 4 IP_OF_SERVER_1 IP_OF_SERVER_2 --export-dir configs -w
```

Now, some files are exported in `./configs`.

Then, distribute these files to corresponding servers.

**Please make sure you have right to login servers via `ssh IP_OF_SERVER`.**

```
cd ./configs

bash run-all.sh
```

This script will distribute configs, run replicas, and collect experiment results into your local directory.

## Testing

### How is the test work flow?

First, we can try to export a basic config file. (This can be optional)
And you can edit further `base_config.json` if you like.

```
cargo r -- --export-path base_config.json
```

Next, You will use `config-gen` to generate all the files for a committee in a single tests.

```
cargo r -- --config base_config.json config-gen --number <NUMBER> IPs_OF_SERVER --export-dir configs -w
```

If you skip the first step, then just run (default config will be used):

```
cargo r -- config-gen --number <NUMBER> IPs_OF_SERVER --export-dir configs -w
```

As the section above, run:

```
cd configs/
bash run-all.sh
```

Then you'll get the results.

### How performance is calculated in this work?

In our implmentation, there are three timestamps for a single transaction.

1. T1: A timestamp when transaction is created.
2. T2: A timestamp when block is packed by consensus node.
3. T3: A timestamp when it's finalized in a block.

End-to-end performance is calculated via T1 and T3, and 
Consensus performance is calculated via T2 and T3.

We mainly focus on e2e performance here, 
and since consensus throughput is same as end-to-end throughput, we calculate the throughput
in consensus instead.

### Some local test methods.

#### Distributes locally

```Bash
cargo r -- config-gen --number 4 localhost --export-dir configs -w
```

#### Single replica via TCP:

```Bash
cargo run
```

#### Replicas via memory network

```Bash
cargo run -- memory-test
```

#### Failure test over memory network

```Bash
cargo run -- fail-test
```

## Possible issues encountered
1. Process automatically exits (no error reported)

This issue is caused by the process automatically exiting after judging that the throughput is stable (this mechanism helps to collect data). The following are abnormal situations that may cause this issue:

a. The process timeout is too long, and the throughput tends to stabilize.

b. When multiple processes are 'pretend_failure' in succession, the total timeout time is too long and the throughput tends to stabilize.

c. The process is automatically killed by the operating system.

d. Throughput changes from rapid growth to slow growth.

All of the above may cause the system to automatically exit the process before achieving optimal performance.
