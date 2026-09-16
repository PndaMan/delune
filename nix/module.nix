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
#
# Through a VPN: either inside a VPN container's network (gluetun and friends),
#
#   services.delune.vpn.container = "gluetun";   # publish 7474 and the Soulseek port on gluetun
#
# or inside a network namespace that only has the VPN (namespaced WireGuard, vopono):
#
#   services.delune.vpn.namespace = "wg";        # /run/netns/wg
self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.delune;
  environment = {
    DELUNE_BIND = cfg.listen;
    DELUNE_DATA_DIR = cfg.dataDir;
    DELUNE_SLSK_PORT = toString cfg.soulseek.port;
  }
  // lib.optionalAttrs (cfg.libraryDir != null) { DELUNE_LIBRARY_DIR = cfg.libraryDir; }
  // lib.optionalAttrs (cfg.namingTemplate != null) { DELUNE_NAMING_TEMPLATE = cfg.namingTemplate; }
  // lib.optionalAttrs (cfg.soulseek.username != null) {
    DELUNE_SLSK_USERNAME = cfg.soulseek.username;
  }
  // lib.optionalAttrs (cfg.navidrome.url != null) {
    DELUNE_NAVIDROME_URL = cfg.navidrome.url;
    DELUNE_NAVIDROME_USERNAME = cfg.navidrome.username;
  };
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

    vpn = {
      container = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "gluetun";
        description = ''
          Run delune inside this container's network (a VPN container such as gluetun), so
          everything it sends, Soulseek included, goes through the VPN and stops if the VPN
          does. delune then runs as a container itself, built from `package`. Publish
          `listen`'s port and `soulseek.port` from the VPN container, set `listen` to
          `0.0.0.0:<port>`, and forward the Soulseek port through the VPN if you can.
        '';
      };
      namespace = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "wg";
        description = "Run delune in the network namespace `/run/netns/<name>`, for example one that only has a WireGuard interface.";
      };
      user = mkOption {
        type = types.str;
        default = "0:0";
        example = "1024:100";
        description = "`uid:gid` delune runs as in container mode. Match the owner of your music folder (NAS shares often need a specific one).";
      };
      volumes = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [ "/mnt/nas/media:/mnt/nas/media:rslave" ];
        description = "Extra mounts in container mode. `dataDir` and `libraryDir` are mounted at their own paths already; use this to mount a whole share instead, so imports can rename rather than copy.";
      };
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Open the Soulseek port, so peers can connect to you directly.";
    };
  };

  config = mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = cfg.vpn.container == null || cfg.vpn.namespace == null;
            message = "services.delune: use vpn.container or vpn.namespace, not both.";
          }
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
      }

      (mkIf (cfg.vpn.container != null) {
        virtualisation.oci-containers.containers.delune = {
          image = "delune:${cfg.package.version}";
          imageStream = pkgs.dockerTools.streamLayeredImage {
            name = "delune";
            tag = cfg.package.version;
            contents = [ pkgs.cacert ];
            config = {
              Cmd = [
                (lib.getExe cfg.package)
                "serve"
              ];
              Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
            };
          };
          user = cfg.vpn.user;
          environment = environment;
          environmentFiles = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
          volumes = [
            "${cfg.dataDir}:${cfg.dataDir}"
          ]
          # rslave: a NAS share mounted on demand (automount) still shows up inside.
          ++ lib.optional (cfg.libraryDir != null) "${cfg.libraryDir}:${cfg.libraryDir}:rslave"
          ++ cfg.vpn.volumes;
          dependsOn = [ cfg.vpn.container ];
          extraOptions = [ "--network=container:${cfg.vpn.container}" ];
        };
        systemd.tmpfiles.rules = [
          "d ${cfg.dataDir} 0750 ${builtins.replaceStrings [ ":" ] [ " " ] cfg.vpn.user} -"
        ];
        # Wait for a music folder on a network share, and keep retrying behind it.
        systemd.services."${config.virtualisation.oci-containers.backend}-delune" = {
          unitConfig.WantsMountsFor = lib.optional (cfg.libraryDir != null) cfg.libraryDir;
          serviceConfig = {
            Restart = lib.mkOverride 90 "always";
            RestartSec = lib.mkOverride 90 "30s";
          };
        };
      })

      (mkIf (cfg.vpn.container == null) {
        systemd.services.delune = {
          description = "delune";
          wantedBy = [ "multi-user.target" ];
          wants = [ "network-online.target" ];

          inherit environment;
          after = [
            "network-online.target"
          ]
          ++ lib.optional (cfg.vpn.namespace != null) "netns@${cfg.vpn.namespace}.service";

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
          }
          // optionalAttrs (cfg.vpn.namespace != null) {
            # Only the namespace's own interfaces: no route around the VPN.
            NetworkNamespacePath = "/run/netns/${cfg.vpn.namespace}";
          };
        };
      })
    ]
  );
}
