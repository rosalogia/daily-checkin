{ lib, config, pkgs, ... }:

let
  cfg = config.services.daily-checkin;
in {
  options.services.daily-checkin = {
    enable = lib.mkEnableOption "daily-checkin background job";

    package = lib.mkOption {
      type = lib.types.package;
      # default to “this crate” built via callPackage
      default = pkgs.callPackage ../. { };
      description = "Package providing the daily-checkin binary.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "daily-checkin";
      description = "User to run the job as.";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = {};
      description = "Extra environment variables for the job.";
    };
  };

  config = lib.mkIf cfg.enable {
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.user;
    };
    users.groups.${cfg.user} = {};

    systemd.services.daily-checkin = {
      description = "Discord bot for daily check-ins and streak tracking";
      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/daily-checkin";
        User = cfg.user;
      };
      environment = cfg.extraEnvironment;
    };
  };
}
