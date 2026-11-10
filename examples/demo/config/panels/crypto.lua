-- Port of widgets/Crypto.tsx (price + bar graph of one symbol, polled from
-- scripts/crypto.py every 5 min) and widgets/rightPanel/components/
-- CryptoViewer.tsx (the right panel's list of tracked symbols).
--
--   W.Crypto{ symbol = "btc", timeframe = "7d", bars = 16 }
--   W.CryptoViewer{}
local W = hw.widget
local common = require("config.panels.common")
local util = require("lib.util")
local Box = common.Box

local SCRIPT = CONFIG_DIR .. "/scripts/crypto.py"
local POLL_MS = 300000
local GRAPH = { "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█" }
local TIMEFRAMES = { "1h", "24h", "7d", "30d", "90d", "1y" }
local ENTRIES_FILE = common.CACHE_DIR .. "/crypto/entries.json"

-- Percentile-ranked bars, like AGS updateGraph.
local function graph(prices, n)
  if #prices == 0 then
    return string.rep(GRAPH[1], n)
  end
  local recent = {}
  for i = math.max(1, #prices - n + 1), #prices do
    table.insert(recent, prices[i])
  end
  while #recent < n do
    table.insert(recent, 1, recent[1] or 0)
  end
  local sorted = { table.unpack(recent) }
  table.sort(sorted)
  local out = {}
  for _, p in ipairs(recent) do
    local rank = 0
    for _, x in ipairs(sorted) do
      if x <= p then
        rank = rank + 1
      end
    end
    out[#out + 1] = GRAPH[math.min(8, math.floor(rank / #sorted * 8) + 1)]
  end
  return table.concat(out)
end

local function trend(prices)
  if #prices < 2 then
    return "neutral"
  end
  return prices[#prices] >= prices[1] and "up" or "down"
end

-- en-US currency format with 2 decimals.
local function money(v)
  local int, frac = string.format("%.2f", v):match("^(%-?%d+)%.(%d+)$")
  int = int:reverse():gsub("(%d%d%d)", "%1,"):reverse():gsub("^(%-?),", "%1")
  return "$" .. int .. "." .. frac
end

hw.define("Crypto", function(props)
  local symbol = props.symbol or "btc"
  local timeframe = props.timeframe or "7d"
  local n = props.bars or 10
  local prices = hw.state({})
  local status = hw.state("loading")

  local function fetch()
    status:set("loading")
    hw.exec_async({ "python", SCRIPT, symbol, timeframe }, function(out, err)
      local ok, parsed = pcall(hw.json.decode, out or "")
      if err or not ok or type(parsed) ~= "table" or type(parsed.prices) ~= "table" then
        status:set("error")
        return
      end
      local list = {}
      for _, p in ipairs(parsed.prices) do
        table.insert(list, tonumber(p.price) or 0)
      end
      prices:set(list)
      status:set("success")
    end)
  end
  local timer = hw.interval(POLL_MS, fetch)

  local cls = prices:map(trend)
  return Box({
    class = cls:map(function(c)
      return "crypto " .. c
    end),
    spacing = 4,
    vertical = props.vertical ~= false,
    on_destroy = function()
      timer:cancel()
    end,
    hw.With(
      prices:map(function(p)
        return #p > 0
      end),
      function(loaded)
        if not loaded then
          return common.Progress({ status = status, class = "crypto-loading", transition = "none" })
        end
        return Box({
          spacing = 2,
          props.show_price ~= false and Box({
            spacing = 2,
            W.Label({ class = "crypto-symbol", label = symbol:upper() }),
            W.Label({
              class = "crypto-price",
              ellipsize = "end",
              label = prices:map(function(p)
                return money(p[#p] or 0)
              end),
            }),
          }) or nil,
          props.show_graph ~= false and W.Label({
            class = cls:map(function(c)
              return "crypto-graph mono " .. c
            end),
            label = prices:map(function(p)
              return graph(p, n)
            end),
            xalign = 0,
          }) or nil,
        })
      end
    ),
  })
end)

-- CryptoViewer -----------------------------------------------------------

local entries = hw.state(util.read_json(ENTRIES_FILE, {}))
local show_form = hw.state(false)
local editing = hw.state(false) -- the entry being edited, or false

local function save(list)
  entries:set(list)
  util.write_json(ENTRIES_FILE, list)
end

local function Form(entry)
  local symbol = (entry and entry.symbol) or "btc"
  local timeframe = hw.state((entry and entry.timeframe) or "7d")
  local show_price = hw.state(entry == nil or entry.showPrice ~= false)
  local show_graph = hw.state(entry == nil or entry.showGraph ~= false)

  local function submit()
    local s = symbol:gsub("^%s+", ""):gsub("%s+$", ""):lower()
    if s == "" then
      common.notify("Crypto Display", "Please enter a valid symbol")
      return
    end
    local new = {
      id = entry and entry.id or tostring(os.time() * 1000),
      symbol = s,
      timeframe = timeframe:peek(),
      showPrice = show_price:peek(),
      showGraph = show_graph:peek(),
    }
    local list = {}
    for _, e in ipairs(entries:peek()) do
      table.insert(list, (entry and e.id == entry.id) and new or e)
    end
    if not entry then
      table.insert(list, new)
    end
    save(list)
    show_form:set(false)
    editing:set(false)
    common.notify("Crypto Display", s:upper() .. " " .. (entry and "updated" or "added") .. " successfully")
  end

  local tf_buttons = { class = "timeframes", spacing = 4 }
  for _, tf in ipairs(TIMEFRAMES) do
    table.insert(
      tf_buttons,
      common.toggle({
        class = "timeframe",
        label = tf,
        vexpand = false,
        active = timeframe:map(function(cur)
          return cur == tf
        end),
        on_toggle = function()
          timeframe:set(tf)
        end,
      })
    )
  end

  local function option(label, state)
    return common.toggle({
      class = "option-toggle",
      label = label,
      active = state,
      on_toggle = function(on)
        state:set(on)
      end,
    })
  end

  return Box({
    class = "form-container crypto-form",
    vertical = true,
    spacing = 8,
    Box({
      vertical = true,
      spacing = 4,
      W.Label({ class = "form-label", label = "Crypto Symbol", halign = "start" }),
      W.Entry({
        text = symbol,
        placeholder = "e.g. btc, eth, sol",
        on_change = function(_, t)
          symbol = t
        end,
        on_activate = submit,
      }),
    }),
    Box({
      vertical = true,
      spacing = 4,
      W.Label({ class = "form-label", label = "Timeframe", halign = "start" }),
      W.Scrolled({ hexpand = true, vscroll = "never", Box(tf_buttons) }),
    }),
    Box({
      vertical = true,
      spacing = 4,
      W.Label({ class = "form-label", label = "Display Options", halign = "start" }),
      Box({ spacing = 8, option("Show Price", show_price), option("Show Graph", show_graph) }),
    }),
    Box({
      class = "form-actions",
      spacing = 8,
      W.Button({ class = "success", label = entry and "✓ Update" or "+ Add Crypto", hexpand = true, on_click = submit }),
      W.Button({
        class = "danger",
        label = "✕ Cancel",
        on_click = function()
          show_form:set(false)
          editing:set(false)
        end,
      }),
    }),
  })
end

local function Item(entry, width)
  return common.hover_actions({
    class = "crypto-entry-eventbox",
    actions = Box({
      class = "crypto-entry-actions",
      W.Button({
        label = "\u{f435}",
        tooltip = "Pin to the bar",
        on_click = function()
          hw.settings:set("crypto.favorite", { symbol = entry.symbol, timeframe = entry.timeframe })
          common.notify("Crypto Display", entry.symbol:upper() .. " pinned to top bar")
        end,
      }),
      W.Button({
        label = "\u{ea73}",
        on_click = function()
          editing:set(entry)
          show_form:set(true)
        end,
      }),
      W.Button({
        label = "✕",
        on_click = function()
          local list = {}
          for _, e in ipairs(entries:peek()) do
            if e.id ~= entry.id then
              table.insert(list, e)
            end
          end
          save(list)
          common.notify("Crypto Display", entry.symbol:upper() .. " removed")
        end,
      }),
    }),
    build = function(actions)
      return Box({
        class = "crypto-entry",
        vertical = true,
        spacing = 6,
        hexpand = true,
        Box({
          class = "crypto-entry-header",
          Box({ class = "crypto-entry-info", W.Label({ class = "crypto-timeframe", label = entry.timeframe .. " timeframe" }) }),
          Box({ hexpand = true }),
          actions,
        }),
        Box({
          class = "crypto-widget-container",
          hw.With(width, function(wd)
            return W.Crypto({
              symbol = entry.symbol,
              timeframe = entry.timeframe,
              show_price = entry.showPrice,
              show_graph = entry.showGraph,
              bars = math.floor((wd or 250) / 15),
            })
          end),
        }),
      })
    end,
  })
end

hw.define("CryptoViewer", function(props)
  local width = common.panel_content_width("right")
  return Box({
    class = common.class("crypto-widget", props.class),
    vertical = true,
    spacing = 5,
    Box({
      class = "header",
      W.Label({ class = "title", label = "Crypto Tracker", hexpand = true, halign = "start" }),
      W.Button({
        class = "add-btn",
        label = show_form:map(function(s)
          return s and "✕" or "+"
        end),
        on_click = function()
          editing:set(false)
          show_form:set(not show_form:peek())
        end,
      }),
    }),
    W.Revealer({
      reveal = show_form,
      transition = "swing_down",
      duration = common.TRANSITION,
      hw.With(editing, function(e)
        return Form(e or nil)
      end),
    }),
    W.Scrolled({
      class = "crypto-list",
      vexpand = true,
      hscroll = "never",
      common.For(entries, function(e)
        return Item(e, width)
      end, function(e)
        return e.id
      end, { vertical = true, spacing = 8 }),
    }),
  })
end)
