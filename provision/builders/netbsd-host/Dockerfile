# The NetBSD host of builders.toml (plan 14.6): a Linux container that runs build.sh. build.sh
# cross-builds from any POSIX host and builds its own reference GCC there, so the container needs
# only a host C and C++ compiler, zlib, and the tools that build.sh and the rb scripts call.
#
# The base is debian:13 at the index digest that builders.toml records. The packages come from the
# Debian archive on the day the image is built. The image records their versions in
# /etc/rb-packages.txt, and the netbsd-host workflow prints them.
FROM debian:13@sha256:9c6e381421d0f48d1b8133e573f33c185ee16cbf505eda9c4a8a4791b25ea8a6
ENV DEBIAN_FRONTEND=noninteractive LC_ALL=C.UTF-8
RUN apt-get update -q \
	&& apt-get install -qy --no-install-recommends \
		bash ca-certificates file g++ gcc git make python3 xz-utils zlib1g-dev zstd \
	&& rm -rf /var/lib/apt/lists/* \
	&& dpkg-query -W -f '${Package} ${Version}\n' | sort > /etc/rb-packages.txt
CMD ["bash"]
