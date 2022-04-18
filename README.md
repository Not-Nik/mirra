<div align="center">
<h1>mirra</h1>
<p><strong>
Manage your mirrors with ease (likely easier than in real-life
[depends on your experience with real-life mirrors])
</strong></p>
</div>

Mirra allows you to create and manage mirror sites that automatically synchronise data between
them. Any changes on a local root mirror will immediately be reflected on any remote mirra nodes.

## Usage

A mirror can be a root and a node for several modules at the same time. A node can query all
available modules from a remote mirra and then decide which one it wants to synchronise.

### Creating a new root mirra

Creating a new mirror with mirra is super easy:

1. Create a new empty directory

```shell
$ mkdir my_mirror
```

2. Put all your data into that
3. Initialise mirra:

```shell
$ ls
my_mirror
$ mirra
mirra name?: <put your name here>
mirra port? [6007]: <optional: put a port here>
# Mirra will now generate some keys, wait until that is done,
# then stop mirra with CTRL+C
```

4. Register your data with mirra.

```shell
$ mirra share my_mirror
```

Done! Running mirra will enable anyone to access your data via the port you specified.
Additionally a web server will run on port 80, to allow users to download files via their browser.

### Mirror an existing mirra

1. `cd` into the folder where you want the module to be stored.
2. Initialise mirra:

```shell
$ mirra
mirra name?: <put your name here>
mirra port? [6007]: <optional: put a port here>
# Mirra will now generate some keys, wait until that is done,
# then stop mirra with CTRL+C
```

4. Sync the remote module

```shell
$ mirra sync remote.mirra.domain[:port] module_name
```

Running mirra will automatically create a directory for your module, load the module and synchronise
any changes from the remote mirra. Any other mirra will also be able to synchronise data from this
module as if the local node were a root mirra. Users will be able to browser the module via their
browser.

## Roadmap

Mirra isn't fully usable yet. This is what's to come:

- [x] Basic usage
- [x] Easier mirra setup
- [ ] (On-hold) Get a remote mirras index via CLI
- [x] Web interface for downloading a mirras data
- [ ] Let a root mirra verify official nodes
- [ ] Automatic redirects based on location

## Protocol

Mirra uses an entirely custom protocol to synchronise changes across hosts.

TODO: Write docs for protocol
