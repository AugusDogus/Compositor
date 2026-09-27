{
  description = "Compositor for Linux";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }: let
    system = "x86_64-linux";
    pkgs = import nixpkgs { inherit system; };
    compositor = pkgs.callPackage ./packaging/linux/compositor-bin.nix {};
  in {
    packages.${system} = {
      compositor-bin = compositor;
      default = compositor;
    };
    apps.${system}.default = {
      type = "app";
      program = "${compositor}/bin/compositor";
      meta.description = "Layered image editor with offline selection models";
    };
  };
}
