{
  description = "Spencer Macro Utilities";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems =
        function:
        nixpkgs.lib.genAttrs supportedSystems (
          system:
          function (import nixpkgs {
            inherit system;
          })
        );
    in
    {
      packages = forAllSystems (pkgs: rec {
        spencer-macro-utilities = pkgs.callPackage ./nix/package.nix { };
        default = spencer-macro-utilities;
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/spencer-macro-utilities";
        };
      });

      checks = forAllSystems (
        pkgs:
        let
          system = pkgs.stdenv.hostPlatform.system;
          moduleConfig = (
            nixpkgs.lib.nixosSystem {
              inherit system;
              modules = [
                self.nixosModules.default
                {
                  system.stateVersion = "25.11";
                  programs.spencer-macro-utilities = {
                    enable = true;
                    inputUsers = [ "smu-test" ];
                  };
                  users.users.smu-test.isNormalUser = true;
                }
              ];
            }
          ).config;
          moduleRules = moduleConfig.services.udev.extraRules;
          moduleCheck =
            assert moduleConfig.security.polkit.enable;
            assert nixpkgs.lib.elem "uinput" moduleConfig.boot.kernelModules;
            assert nixpkgs.lib.elem "smu-input" moduleConfig.users.users.smu-test.extraGroups;
            assert nixpkgs.lib.hasInfix ''KERNEL=="uinput"'' moduleRules;
            pkgs.writeText "spencer-macro-utilities-module-check.json" (
              builtins.toJSON {
                package = toString moduleConfig.programs.spencer-macro-utilities.package;
                inputGroup = moduleConfig.programs.spencer-macro-utilities.inputGroup;
                inputUserGroups = moduleConfig.users.users.smu-test.extraGroups;
                kernelModules = moduleConfig.boot.kernelModules;
                polkitEnabled = moduleConfig.security.polkit.enable;
                udevRules = moduleRules;
              }
            );
        in
        {
          package = self.packages.${system}.default;
          nixos-module = moduleCheck;
        }
      );

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = with pkgs; [
            clang-tools
            gdb
          ];
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt-rfc-style);

      overlays.default = final: _previous: {
        spencer-macro-utilities = final.callPackage ./nix/package.nix { };
      };

      nixosModules.default = import ./nix/module.nix { inherit self; };
      nixosModules.spencer-macro-utilities = self.nixosModules.default;
    };
}
