{ lib, appimageTools, fetchurl }:
let
  pname = "compositor";
  version = "0.6.1";
  src = fetchurl {
    url = "https://github.com/AugusDogus/Compositor/releases/download/v${version}/Compositor-${version}-x86_64.AppImage";
    sha256 = "46c8f405e53b18aaaf1513ebc33563505ef75b709f6a19b1d7d3fa881cad5931";
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
