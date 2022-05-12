#!/bin/bash

for MONTHS in $(seq 120)
do
            ts_start=$(date -d "`date +%Y%m01` -$MONTHS month" +%Y-%m-%d)
            ts_end=$(date -d "`date +%Y%m01` -$((MONTHS-1)) month" +%Y-%m-%d)
            command="peer-stats-bootstrap --ts-start $ts_start --ts-end $ts_end --output-dir /data/bgpkit/public/peer-stats/ --only-daily"
            echo $command
            eval $command
done