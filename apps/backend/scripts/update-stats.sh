#!/bin/sh
# How many installations of VibeSSH ran, day by day.
#
# Reads the table `updates.rs` fills - see migrations/0013 for what is in it
# and, more to the point, what deliberately is not. Run on the backend host:
#
#   sudo sh /opt/vibessh/src/apps/backend/scripts/update-stats.sh
#   sudo sh /opt/vibessh/src/apps/backend/scripts/update-stats.sh 60
#
# A script in the repository rather than a command somebody has to remember,
# because the alternative is a line of nested quoting that gets retyped
# slightly wrong every time - and because this way it is reviewed, and it
# arrives on the host with every deploy.
#
# There is no HTTP endpoint for this on purpose. Anyone who can read these
# numbers already has SSH to the machine, and an endpoint would have meant a
# token to keep safe for a convenience nobody asked for.
set -eu

DAYS=${1:-30}
case "$DAYS" in
    ''|*[!0-9]*) echo "usage: $0 [days]  (days must be a number)" >&2; exit 2 ;;
esac

# `-x`-free, aligned output: this is read by a person, not parsed.
docker exec vibessh-postgres-1 sh -c "psql -U \"\$POSTGRES_USER\" -d \"\$POSTGRES_DB\" -c \"
SELECT day                                  AS \\\"dzien\\\",
       count(DISTINCT client_day_hash)      AS \\\"instalacje\\\",
       sum(checks)                          AS \\\"sprawdzen\\\",
       string_agg(DISTINCT version, ', ' ORDER BY version)   AS \\\"wersje\\\",
       string_agg(DISTINCT platform, ', ' ORDER BY platform) AS \\\"systemy\\\"
FROM update_checks
WHERE day > current_date - $DAYS
GROUP BY day
ORDER BY day DESC;\""

echo
echo "instalacje = ile roznych maszyn odezwalo sie tego dnia."
echo "Ta sama maszyna nazajutrz jest nie do powiazania z dniem poprzednim -"
echo "sol zmienia sie codziennie, wiec retencji z tego nie policzysz."
