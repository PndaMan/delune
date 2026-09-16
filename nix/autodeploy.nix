# Keeps a NixOS machine on the latest of its flake: follows the repo's upstream
# and chosen inputs, deploys what changed, checks the machine is healthy, and
# switches back to the last good system when it isn't.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.autodeploy;

  script = pkgs.writeShellApplication {
    name = "autodeploy";
    runtimeInputs = with pkgs; [
      bash
      coreutils
      curl
      diffutils
      gawk
      git
      gnugrep
      jq
      config.nix.package
      systemd
      util-linux
    ];
    text = builtins.readFile ./autodeploy.sh;
  };
in
{
  options.services.autodeploy = {
    enable = lib.mkEnableOption "deploying this machine's flake whenever it changes, with automatic rollback";

    repo = lib.mkOption {
      type = lib.types.str;
      example = "/root/nixos-config";
      description = "The flake repo on this machine that describes it.";
    };

    host = lib.mkOption {
      type = lib.types.str;
      default = config.networking.hostName;
      defaultText = lib.literalExpression "config.networking.hostName";
      description = "Which `nixosConfigurations` entry to deploy.";
    };

    inputs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "delune" ];
      description = ''
        Flake inputs (github: ones) to follow: a push to one is picked up and
        deployed. Other inputs stay where the lock file has them.
      '';
    };

    pull = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Fast-forward the repo to its upstream branch, so a push to the repo
        itself deploys too. Skipped while the tree has uncommitted changes.
      '';
    };

    requireChecks = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Deploy an input's commit only once its GitHub checks have passed.";
    };

    githubTokenFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "A GitHub token for reading checks, for private repos or a higher rate limit.";
    };

    interval = lib.mkOption {
      type = lib.types.str;
      default = "2min";
      description = "How often to look for changes. A look that finds nothing is cheap.";
    };

    units = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "podman-delune.service" ];
      description = "Units that must be active after a deploy.";
    };

    checks = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "curl -fsS http://127.0.0.1:7474/api/v1/health" ];
      description = "Commands that must succeed after a deploy.";
    };

    settleSeconds = lib.mkOption {
      type = lib.types.ints.positive;
      default = 20;
      description = "Wait between health checks.";
    };

    healthTries = lib.mkOption {
      type = lib.types.ints.positive;
      default = 6;
      description = "How many times to check before rolling back.";
    };

    commitLock = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Commit the updated flake.lock (only that file) once a deploy is healthy.";
    };

    push = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Push that commit upstream.";
    };

    ntfyUrlFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/run/secrets/autodeploy-ntfy";
      description = "A file holding an ntfy topic URL, to hear about deploys and rollbacks.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.autodeploy = {
      description = "Deploy this machine's flake and roll back if it breaks";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      # A deploy switches this machine, which must not stop the deploy doing it.
      restartIfChanged = false;
      stopIfChanged = false;
      environment = {
        REPO = cfg.repo;
        HOST = cfg.host;
        INPUTS = lib.concatStringsSep " " cfg.inputs;
        PULL = if cfg.pull then "1" else "0";
        REQUIRE_CI = if cfg.requireChecks then "1" else "0";
        GITHUB_TOKEN_FILE = toString cfg.githubTokenFile;
        UNITS = lib.concatStringsSep " " cfg.units;
        CHECKS_FILE = pkgs.writeText "autodeploy-checks" (lib.concatLines cfg.checks);
        SETTLE_SECONDS = toString cfg.settleSeconds;
        HEALTH_TRIES = toString cfg.healthTries;
        COMMIT_LOCK = if cfg.commitLock then "1" else "0";
        PUSH = if cfg.push then "1" else "0";
        NTFY_URL_FILE = toString cfg.ntfyUrlFile;
        NIX_CONFIG = "experimental-features = nix-command flakes";
        HOME = "/root";
      };
      serviceConfig = {
        Type = "oneshot";
        ExecStart = lib.getExe script;
        StateDirectory = "autodeploy";
        # Building can take a while; a deploy that hangs is stopped and checked next run.
        TimeoutStartSec = "2h";
      };
    };

    systemd.timers.autodeploy = {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "3min";
        OnUnitInactiveSec = cfg.interval;
        Persistent = false;
      };
    };
  };
}
