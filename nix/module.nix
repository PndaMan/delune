# NixOS module for delune.
#
#   services.delune = {
#     enable = true;
#     libraryDir = "/srv/music";          # the folder Navidrome scans
#     soulseek.username = "your-name";
#     navidrome.url = "http://127.0.0.1:4533";
#     navidrome.username = "admin";
#     environmentFile = "/run/secrets/delune.env";  # DELUNE_SLSK_PASSWORD=… DELUNE_NAVIDROME_PASSWORD=…
#     openFirewall = true;                # the Soulseek port, so peers can reach you
#   };
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.delune;
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    optionalAttrs
    types
    ;
in
{
  options.services.delune = {
    enable = mkEnableOption "delune, a Soulseek client that files music into Navidrome";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.delune;
      defaultText = lib.literalExpression "delune.packages.\${system}.delune";
      description = "The delune package to run.";
    };

    listen = mkOption {
      type = types.str;
      default = "127.0.0.1:7474";
      description = "Address the web UI and API listen on. Put a reverse proxy with HTTPS in front for remote use.";
    };

    dataDir = mkOption {
      type = types.path;
      default = "/var/lib/delune";
      description = "Where delune keeps downloads under review, accounts and settings.";
    };

    libraryDir = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/srv/music";
      description = "The music folder Navidrome scans. Approved imports are moved here.";
    };

    namingTemplate = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "{album_artist}/[{year} - ]{album}/{track} - {title}";
      description = "How imported files are named. The default matches common library layouts.";
    };

    user = mkOption {
      type = types.str;
      default = "delune";
      description = "User delune runs as.";
    };

    group = mkOption {
      type = types.str;
      default = "delune";
      description = "Group delune runs as. Give it write access to the library, for example Navidrome's group.";
    };

    soulseek = {
      username = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Soulseek account name. A new name is registered the first time it logs in.";
      };
      port = mkOption {
        type = types.port;
        default = 2234;
        description = "Port other Soulseek users connect to.";
      };
    };

    navidrome = {
      url = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "http://127.0.0.1:4533";
        description = "Navidrome's address. Turns on sign-in with Navidrome accounts, library checks and rescans.";
      };
      username = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "A Navidrome admin account delune uses for rescans.";
      };
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/run/secrets/delune.env";
      description = ''
        File with secrets, kept out of the Nix store: `DELUNE_SLSK_PASSWORD=…` and
        `DELUNE_NAVIDROME_PASSWORD=…`.
      '';
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Open the Soulseek port, so peers can connect to you directly.";
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = (cfg.navidrome.url == null) == (cfg.navidrome.username == null);
        message = "services.delune: set navidrome.url and navidrome.username together.";
      }
    ];

    users.users = optionalAttrs (cfg.user == "delune") {
      delune = {
        isSystemUser = true;
        group = cfg.group;
        home = cfg.dataDir;
      };
    };
    users.groups = optionalAttrs (cfg.group == "delune") { delune = { }; };

    networking.firewall.allowedTCPPorts = lib.optional cfg.openFirewall cfg.soulseek.port;

    systemd.services.delune = {
      description = "delune";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      environment = {
        DELUNE_BIND = cfg.listen;
        DELUNE_DATA_DIR = cfg.dataDir;
        DELUNE_SLSK_PORT = toString cfg.soulseek.port;
      }
      // optionalAttrs (cfg.libraryDir != null) { DELUNE_LIBRARY_DIR = cfg.libraryDir; }
      // optionalAttrs (cfg.namingTemplate != null) { DELUNE_NAMING_TEMPLATE = cfg.namingTemplate; }
      // optionalAttrs (cfg.soulseek.username != null) { DELUNE_SLSK_USERNAME = cfg.soulseek.username; }
      // optionalAttrs (cfg.navidrome.url != null) {
        DELUNE_NAVIDROME_URL = cfg.navidrome.url;
        DELUNE_NAVIDROME_USERNAME = cfg.navidrome.username;
      };

      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} serve";
        EnvironmentFile = lib.mkIf (cfg.environmentFile != null) cfg.environmentFile;
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = 5;
        # New files are group-writable, so Navidrome and people in the group can manage them.
        UMask = "0002";

        StateDirectory = lib.mkIf (cfg.dataDir == "/var/lib/delune") "delune";
        ReadWritePaths = [ cfg.dataDir ] ++ lib.optional (cfg.libraryDir != null) cfg.libraryDir;

        # Hardening: delune needs the network and its two folders, nothing else.
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateDevices = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        CapabilityBoundingSet = "";
      };
    };
  };
}
