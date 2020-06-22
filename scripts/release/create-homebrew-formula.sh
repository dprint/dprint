#!/bin/sh
shafile=$1
version=$2
sha=`shasum -a 256 $shafile | cut -d' ' -f1`
cat scripts/release/assets/dprint-formula.txt | sed -e "s/SHA/$sha/g" -e "s/VERSION/$version/g"
