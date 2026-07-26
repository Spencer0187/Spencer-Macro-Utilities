{ self }:

{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.spencer-macro-utilities;
in
{
  options.programs.spencer-macro-utilities = {
    enable = lib.mkEnableOption "Spencer Macro Utilities";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "inputs.smu.packages.\${pkgs.stdenv.hostPlatform.system}.default";
      description = "The Spencer Macro Utilities package to install.";
    };

    inputGroup = lib.mkOption {
      type = lib.types.strMatching "^[a-z_][a-z0-9_-]*$";
      default = "smu-input";
      description = "Group granted access to Linux input and uinput devices.";
    };

    inputUsers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "alice" ];
      description = "Existing desktop users to add to the input device group.";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelModules = [ "uinput" ];

    environment.systemPackages = [
      cfg.package
      pkgs.iproute2
      pkgs.iptables
      pkgs.kmod
      pkgs.polkit
    ];

    security.polkit.enable = true;

    users.groups.${cfg.inputGroup} = { };
    users.users = lib.genAttrs cfg.inputUsers (_user: {
      extraGroups = [ cfg.inputGroup ];
    });

    services.udev.extraRules = ''
      KERNEL=="uinput", MODE="0660", GROUP="${cfg.inputGroup}", TAG+="uaccess", OPTIONS+="static_node=uinput"
      SUBSYSTEM=="input", KERNEL=="event*", MODE="0660", GROUP="${cfg.inputGroup}", TAG+="uaccess"
    '';
  };
}
