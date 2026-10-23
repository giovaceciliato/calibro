# RPM package for Fedora (40 or newer). Built from a prebuilt release binary by
# packaging/build-linux-packages.sh — it does not compile Rust inside rpmbuild.
Name:           calibro
Version:        %{calibro_version}
Release:        1%{?dist}
Summary:        Precision 3D CAD written in Rust
License:        Apache-2.0
URL:            https://github.com/giovaceciliato/calibro
ExclusiveArch:  x86_64
AutoReqProv:    yes

# Libraries the app opens at runtime (not visible to the automatic dependency scan).
Requires:       libvulkan.so.1()(64bit)
Requires:       libEGL.so.1()(64bit)
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-egl.so.1()(64bit)
Requires:       libX11.so.6()(64bit)
Requires:       libX11-xcb.so.1()(64bit)
Requires:       libXcursor.so.1()(64bit)
Requires:       libXi.so.6()(64bit)
Requires:       libdbus-1.so.3()(64bit)
Recommends:     mesa-vulkan-drivers
Recommends:     xdg-desktop-portal

%description
Calibro is an open-source, history-based parametric 3D CAD application: sketch with
constraints and dimensions, extrude and revolve solids, combine them with booleans,
edit any step of the history, and export STEP, STL, OBJ and glTF.

%install
install -Dm0755 %{_sourcedir}/calibro %{buildroot}%{_bindir}/calibro
install -Dm0644 %{_sourcedir}/io.github.giovaceciliato.Calibro.desktop %{buildroot}%{_datadir}/applications/io.github.giovaceciliato.Calibro.desktop
install -Dm0644 %{_sourcedir}/io.github.giovaceciliato.Calibro.xml %{buildroot}%{_datadir}/mime/packages/io.github.giovaceciliato.Calibro.xml
install -Dm0644 %{_sourcedir}/io.github.giovaceciliato.Calibro.metainfo.xml %{buildroot}%{_datadir}/metainfo/io.github.giovaceciliato.Calibro.metainfo.xml
for s in 64 128 256; do
  install -Dm0644 %{_sourcedir}/calibro-$s.png %{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps/io.github.giovaceciliato.Calibro.png
done
install -Dm0644 %{_sourcedir}/LICENSE %{buildroot}%{_licensedir}/%{name}/LICENSE
install -Dm0644 %{_sourcedir}/README.md %{buildroot}%{_docdir}/%{name}/README.md

%files
%license %{_licensedir}/%{name}/LICENSE
%doc %{_docdir}/%{name}/README.md
%{_bindir}/calibro
%{_datadir}/applications/io.github.giovaceciliato.Calibro.desktop
%{_datadir}/mime/packages/io.github.giovaceciliato.Calibro.xml
%{_datadir}/metainfo/io.github.giovaceciliato.Calibro.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/io.github.giovaceciliato.Calibro.png

%changelog
* Fri Oct 23 2026 Calibro authors - 0.1.0-1
- First release
