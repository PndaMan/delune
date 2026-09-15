# A VM that runs delune from the module and checks it serves its API and web UI.
self:
{
  name = "delune";

  nodes.machine = {
    imports = [ self.nixosModules.delune ];
    services.delune = {
      enable = true;
      libraryDir = "/srv/music";
    };
    systemd.tmpfiles.rules = [ "d /srv/music 0775 delune delune -" ];
  };

  testScript = ''
    machine.wait_for_unit("delune.service")
    machine.wait_for_open_port(7474)
    machine.succeed("curl -sf http://127.0.0.1:7474/api/v1/health | grep -q '\"status\":\"ok\"'")
    machine.succeed("curl -sf http://127.0.0.1:7474/ | grep -q '<div id=\"root\">'")
    machine.succeed("curl -sf http://127.0.0.1:7474/manifest.webmanifest | grep -q delune")
  '';
}
