# SPDK .deb package creation
# Uses DESTDIR staging with dpkg-deb - no extra tools required
#
# Required variables (must be set before including this file):
#   spdk_SOURCE_DIR - Path to SPDK source directory
#
# Provides target:
#   spdk_deb - Creates .deb package (depends on build_spdk)

set(SPDK_VERSION "26.01")
set(SPDK_PKG_DIR ${CMAKE_CURRENT_BINARY_DIR}/spdk-pkg)
set(SPDK_PKG_TEMPLATES_DIR ${CMAKE_CURRENT_LIST_DIR}/pkg)

# Generate control file with version substitution
configure_file(
    ${SPDK_PKG_TEMPLATES_DIR}/control.in
    ${CMAKE_CURRENT_BINARY_DIR}/spdk-control.txt
    @ONLY
)

add_custom_target(spdk_deb
    # Clean previous package directory
    COMMAND ${CMAKE_COMMAND} -E remove_directory ${SPDK_PKG_DIR}
    COMMAND ${CMAKE_COMMAND} -E make_directory ${SPDK_PKG_DIR}/DEBIAN
    COMMAND ${CMAKE_COMMAND} -E make_directory ${SPDK_PKG_DIR}/etc/ld.so.conf.d
    COMMAND ${CMAKE_COMMAND} -E make_directory ${SPDK_PKG_DIR}/etc/profile.d
    # Install SPDK to staging directory (skip Python which doesn't respect DESTDIR)
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/lib install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/module install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/shared_lib install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/include install
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/app install
    # Install DPDK to staging directory (uses dpdkbuild which reconfigures prefix to /opt/spdk)
    COMMAND ${CMAKE_COMMAND} -E env DESTDIR=${SPDK_PKG_DIR} $(MAKE) -C ${spdk_SOURCE_DIR}/dpdkbuild install
    # Install bundled ISA-L libraries if built (uses autotools install which includes .pc files)
    COMMAND test -f ${spdk_SOURCE_DIR}/isa-l/Makefile && $(MAKE) -C ${spdk_SOURCE_DIR}/isa-l DESTDIR=${SPDK_PKG_DIR} install
    COMMAND test -f ${spdk_SOURCE_DIR}/isa-l-crypto/Makefile && $(MAKE) -C ${spdk_SOURCE_DIR}/isa-l-crypto DESTDIR=${SPDK_PKG_DIR} install
    # Copy control file (generated), ldconfig conf, profile.d script, and maintainer scripts
    COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_BINARY_DIR}/spdk-control.txt ${SPDK_PKG_DIR}/DEBIAN/control
    COMMAND ${CMAKE_COMMAND} -E copy ${SPDK_PKG_TEMPLATES_DIR}/spdk.conf ${SPDK_PKG_DIR}/etc/ld.so.conf.d/spdk.conf
    COMMAND ${CMAKE_COMMAND} -E copy ${SPDK_PKG_TEMPLATES_DIR}/spdk.sh ${SPDK_PKG_DIR}/etc/profile.d/spdk.sh
    COMMAND ${CMAKE_COMMAND} -E copy ${SPDK_PKG_TEMPLATES_DIR}/postinst ${SPDK_PKG_DIR}/DEBIAN/postinst
    COMMAND ${CMAKE_COMMAND} -E copy ${SPDK_PKG_TEMPLATES_DIR}/postrm ${SPDK_PKG_DIR}/DEBIAN/postrm
    # dpkg-deb requires the control directory to be <=0775; cmake -E make_directory
    # may create it as 0777 depending on umask/CMake version, so normalize it here.
    COMMAND chmod 755 ${SPDK_PKG_DIR}/DEBIAN
    COMMAND chmod 755 ${SPDK_PKG_DIR}/DEBIAN/postinst ${SPDK_PKG_DIR}/DEBIAN/postrm
    # Build the .deb package
    COMMAND dpkg-deb --build ${SPDK_PKG_DIR} ${CMAKE_CURRENT_BINARY_DIR}/spdk_${SPDK_VERSION}_amd64.deb
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
    COMMENT "Creating SPDK .deb package (staged, no system install)"
    DEPENDS build_spdk
)
