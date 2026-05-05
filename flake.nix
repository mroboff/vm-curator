{
  description = "vm-curator - A TUI application to manage QEMU VM library";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      # Helper function to generate attributes for each system
      forAllSystems =
        function: nixpkgs.lib.genAttrs supportedSystems (system: function nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          cargoToml = fromTOML (builtins.readFile ./Cargo.toml);
          pname = cargoToml.package.name;
          inherit (cargoToml.package) version;
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            inherit pname version;
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              udev
            ];

            meta = with pkgs.lib; {
              inherit (cargoToml.package) description;
              inherit (cargoToml.package) homepage;
              license = licenses.mit;
              mainProgram = "vm-curator";
            };
          };
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            udev
            cargo
            rustc
            rustfmt
            clippy
            rust-analyzer
          ];
        };
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.system}.default}/bin/vm-curator";
        };
      });
    };
}