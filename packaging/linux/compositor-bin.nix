{ lib, appimageTools, fetchurl }:
let
  pname = "compositor";
  version = "0.7.0";
  src = fetchurl {
    url = "https://github.com/AugusDogus/Compositor/releases/download/v${version}/Compositor-${version}-x86_64.AppImage";
    sha256 = "c13958486795d6e80e683f9cda073d2f1734b2791940454c75ed4cdc421763e7";
  };
  contents = appimageTools.extract {
    inherit pname version src;
    postExtract = ''
      touch "$out/usr/share/compositor/system-package"
    '';
  };
in appimageTools.wrapAppImage {
  inherit pname version src contents;
  extraPkgs = pkgs: [ pkgs.mesa ];
  extraInstallCommands = ''
    install -Dm644 ${contents}/compositor.desktop "$out/share/applications/compositor.desktop"
    substituteInPlace "$out/share/applications/compositor.desktop" \
      --replace-fail 'Exec=compositor %F' "Exec=$out/bin/compositor %F"
    install -Dm644 ${contents}/compositor.png "$out/share/icons/hicolor/256x256/apps/compositor.png"
    install -Dm644 ${contents}/usr/share/metainfo/io.github.AugusDogus.Compositor.metainfo.xml \
      "$out/share/metainfo/io.github.AugusDogus.Compositor.metainfo.xml"
  '';
  meta = {
    description = "Layered image editor with bundled offline selection models";
    homepage = "https://github.com/AugusDogus/Compositor";
    license = lib.licenses.mit;
    platforms = [ "x86_64-linux" ];
    mainProgram = "compositor";
  };
}
