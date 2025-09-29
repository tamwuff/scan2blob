all:

install:
	cargo build --release
	mkdir -p /usr/local/sbin
	install -svm 555 target/release/scan2blob /usr/local/sbin/scan2blob
	mkdir -p /usr/local/etc/syslog.d
	install -vm 444 syslog.d/scan2blob.conf /usr/local/etc/syslog.d/scan2blob.conf
	mkdir -p /usr/local/etc/newsyslog.conf.d
	install -vm 444 newsyslog.conf.d/scan2blob.conf /usr/local/etc/newsyslog.conf.d/scan2blob.conf
	newsyslog -vNC /var/log/scan2blob
	mkdir -p /usr/local/etc/rc.d
	install -vm 555 rc.d/scan2blob /usr/local/etc/rc.d/scan2blob
