# `writeBunScriptBin` - Create Bun $ Shell Scripts

`writeBunScriptBin` is useful for creating once off [$ Shell](https://bun.sh/docs/runtime/shell) scripts, in a similar manner to [`writeShellScriptBin`](https://nixos.org/manual/nixpkgs/unstable/#trivial-builder-writeShellScriptBin) in nixpkgs.

## Example

An example bun script for printing "Hello World" might look like:

```nix
writeBunScriptBin {
  name = "hello-world";
  text = ''
    import { $ } from "bun";

    await $`echo "Hello World!"`;
  '';
};
```

## Arguments

The full list of accepted arguments is:

| Argument | Purpose                                   |
| -------- | ----------------------------------------- |
| `name`   | The name to give the binary of the script |
| `text`   | Textual contents of the script            |
