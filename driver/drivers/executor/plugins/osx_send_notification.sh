#!/usr/bin/env bash

EVENT=$1
ACTIVE_MINUTES=$2
DEBT=$3

TITLE=""
MESSAGE="Active Minutes: ${ACTIVE_MINUTES}; Debt: ${DEBT}"

case ${EVENT} in
"Normal")
    TITLE="No debt this time. Well done!"
    MESSAGE="You've fullfiled your hourly norm.\nActive minutes: ${ACTIVE_MINUTES}"
;;
"DebtCollection")
    TITLE="Hey! Go ahead and move your arse!"
;;
"DebtCollectionPaused")
    TITLE="Relax for a bit, but remember: I'm watching you!"
;;
esac

osascript -e "display notification \"$MESSAGE\" with title \"$TITLE\""
