local threads = {}

function setup(thread)
    table.insert(threads, thread)
    thread:set("tid", table.getn(threads))
end

function init(args)
    responses = {}
end

wrk.method = "GET"
wrk.path = "/"

function response(status, headers, body)
    if responses[status] == nil then
        responses[status] = 1
    else
        responses[status] = responses[status] + 1
    end
end

function done(summary, latency, requests)
    print("wrk is done. response code counts:")
    local freqs = {}
    for _, thread in pairs(threads) do
        for code, freq in pairs(thread:get("responses")) do
            if freqs[code] == nil then
                freqs[code] = freq
            else
                freqs[code] = freqs[code] + freq
            end
        end
    end
    for code, freq in pairs(freqs) do
        print(code, freq)
    end
end
