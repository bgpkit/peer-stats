import json
import time

from bgpkit import Parser

if __name__ == '__main__':

    # let info = parse_rib_file("http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2",
    parser = Parser(url="http://archive.routeviews.org/route-views.sg/bgpdata/2022.02/RIBS/rib.20220205.1800.bz2")

    peers_map = {}

    print("start", time.time())

    for elem in parser:
        ip = elem.peer_ip
        if ip not in peers_map:
            peers_map[ip] = {
                "asn": elem.peer_asn,
                "ip": ip,
                "v4_pfxs": set(),
                "v6_pfxs": set(),
                "connected_asns": set(),
            }

        info = peers_map[ip]

        hops = elem.as_path.split(" ")
        if len(hops) > 1:
            connected = hops[1]
        else:
            connected = None

        if connected and connected not in info["connected_asns"]:
            info["connected_asns"].add(connected)

        if ":" in elem.prefix:
            if elem.prefix not in info["v6_pfxs"]:
                info["v6_pfxs"].add(elem.prefix)
        else:
            if elem.prefix not in info["v4_pfxs"]:
                info["v4_pfxs"].add(elem.prefix)

    res = {
        "collector": "route-views.sg",
        "peers": {},
        "project": "route-views",
        "rib_dump_url": "rib.20220205.1800.bz2"
    }

    res_2 = {
        "27.111.228.122": [],
        "27.111.228.123": []
    }

    for ip in peers_map:
        data = peers_map[ip]
        res["peers"][ip] = {
            "asn": data["asn"],
            "ip": ip,
            "num_v4_pfxs": len(data["v4_pfxs"]),
            "num_v6_pfxs": len(data["v6_pfxs"]),
            "num_connected_asn": len(data["connected_asns"]),
        }
        if ip in res_2:
            res_2[ip]= data["connected_asns"]


    print("end", time.time())

    with open("peer_info_example_py.json", "w") as of:
        of.write(json.dumps(res, indent=4))


    res2 = json.load(open("peer_info_example.json"))

    for ip in res_2:
        with open(f"connected.json","w") as of:
            json.dumps(list(res_2[ip]), indent=4)