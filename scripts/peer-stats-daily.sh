#!/bin/bash

ts_start=$(date -ud "`date +%Y%m%d` -3 day" +%Y-%m-%d)
ts_end=$(date -ud "`date +%Y%m%d` +1 day" +%Y-%m-%d)
command="/usr/local/bin/peer-stats-bootstrap --ts-start $ts_start --ts-end $ts_end --output-dir /data/bgpkit/public/peer-stats/ --only-daily"
echo $command
eval $command