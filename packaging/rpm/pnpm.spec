%global debug_package %{nil}
# The binary is a Node.js single-executable application: the default post-install
# processing strips it, which cuts the injected payload out of the executable.
%global __os_install_post %{nil}
# RED OS 8 carries rpm 4.14, which cannot read the zstd payload that rpm 4.19
# writes by default.
%define _binary_payload w9.gzdio

%global pnpm_root %{_prefix}/lib/pnpm

Name:           pnpm
Version:        %{pnpm_version}
Release:        1%{?dist}
Summary:        Fast, disk space efficient package manager
License:        MIT
URL:            https://pnpm.io
Source0:        pnpm-linux-x64.tar.gz
Source1:        pnpm.sh
ExclusiveArch:  x86_64

# The payload is a prebuilt bundle carrying its own Node.js runtime and its own
# native addons; generated dependencies would describe that bundle rather than
# what the package needs from the system.
AutoReqProv:    no
Requires:       glibc >= 2.28
Requires:       libatomic
Requires:       ca-certificates

%description
pnpm is a fast, disk space efficient package manager for Node.js.

This package ships the standalone build that https://get.pnpm.io/install.sh
installs: the CLI is bundled with its own Node.js runtime, so the system needs
no Node.js of its own, and pnpm can install one on demand.

%prep
%setup -q -c -n %{name}-%{version}

%install
install -d %{buildroot}%{pnpm_root}
cp -a pnpm dist %{buildroot}%{pnpm_root}/
install -d %{buildroot}%{_bindir}
ln -s %{pnpm_root}/pnpm %{buildroot}%{_bindir}/pnpm
install -D -m 0644 %{SOURCE1} %{buildroot}%{_sysconfdir}/profile.d/pnpm.sh

%files
%{pnpm_root}
%{_bindir}/pnpm
%config(noreplace) %{_sysconfdir}/profile.d/pnpm.sh

%changelog
