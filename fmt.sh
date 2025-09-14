#!/bin/sh
max_width=300
while [ $max_width -ge 79 ]; do
	echo "max_width = $max_width" > .rustfmt.toml
	max_width=`expr $max_width - 1`
	cargo fmt
done
