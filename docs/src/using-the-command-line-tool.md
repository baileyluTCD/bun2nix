# The Command Line Tool

## Recommended Usage

The recommended way to use `bun2nix` is by adding it to your `package.json` to automatically run after any package management option:

```json
"scripts": {
    "postinstall": "bun2nix -o bun.nix"
}
```

However, if you run without the `-o` flag it will produce a text output over stdout similar to other `lang2nix` tools, hence if you enforce formatting rules in your repository it is likely a good idea to pass it through a formatter before writing the file.

## Cache

`bun2nix` currently uses an sqlite cache for storing prefetched hashes which has to be cleared manually with `bun2nix cache clear`. 

> This is likely to change soon, see [here](https://github.com/baileyluTCD/bun2nix/issues/2) for details.

## Options

Currently, the options available from the command line tool are as follows:

```
Convert Bun (v1.2+) packages to Nix expressions

Usage: bun2nix [OPTIONS] [COMMAND]

Commands:
  cache  Cache related subcommands
  help   Print this message or the help of the given subcommand(s)

Options:
  -l, --lock-file <LOCK_FILE>
          The Bun (v1.2+) lockfile to use to produce the Nix expression
          
          [default: ./bun.lock]

  -o, --output-file <OUTPUT_FILE>
          The output file to write to - if no file location is provided, print to stdout instead

  -c, --cache-location <CACHE_LOCATION>
          The sqlite database to use as the cache - will be created if it does not exist.
          
          Default value of <system cache directory>/bun2nix is assigned when no value is passed to `cache_location`.
          
          [default: ~/.cache/bun2nix]

      --disable-cache
          Disable usage of the cache entirely

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

