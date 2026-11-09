-- Port of class/BooruImage.class.tsx, widgets/leftPanel/components/
-- BooruViewer.tsx and widgets/rightPanel/components/Waifu.tsx.
--
--   W.BooruViewer{}   -- left panel: browse Wallhaven
--   W.Waifu{}         -- right panel: settings waifuWidget.current
--
-- Images are plain tables as stored in settings (id, width, height, api,
-- tags, extension, url, preview); scripts/booru.py fetches posts and tags.
-- Bookmarks live in settings `booru.bookmarks` (written here, not by the
-- script, so the settings store stays the only writer). Videos show their
-- preview; "open" plays them in mpv.
local W = hw.widget
local common = require("config.panels.common")
local bar_common = require("config.bar.common")
local defaults = require("config.panels.defaults")
local Box = common.Box

local SCRIPT = CONFIG_DIR .. "/scripts/booru.py"
local BOORU_DIR = common.CACHE_DIR .. "/booru"
local MAX_AVATAR_SIZE = 500 * 1024
local RATING = "-?rating:explicit"
local q = common.sh_quote

local M = {}

-- Image helpers -----------------------------------------------------------

local function api_value(img)
  return (img.api and img.api.value) or "custom"
end

function M.image_path(img)
  return string.format("%s/%s/images/%s.%s", BOORU_DIR, api_value(img), img.id, img.extension or "jpg")
end

function M.preview_path(img)
  return string.format("%s/%s/previews/%s.%s", BOORU_DIR, api_value(img), img.id, img.extension or "jpg")
end

local VIDEO = { mp4 = true, webm = true, mkv = true, gif = true }
function M.is_video(img)
  return VIDEO[(img.extension or ""):lower()] == true
end

function M.is_zip(img)
  return (img.extension or ""):lower() == "zip"
end

local function curl(img, url, path)
  local args = { "curl", "-fsSL", "-o", path }
  if api_value(img) == "gelbooru" then
    for _, a in ipairs({ "-H", "Referer: https://gelbooru.com/", "-A", "Mozilla/5.0" }) do
      table.insert(args, a)
    end
  end
  table.insert(args, url)
  return args
end

local function download(img, url, path, cb)
  if common.file_exists(path) then
    cb(true)
    return
  end
  if not url or url == "" then
    cb(false, "Media URL is not available")
    return
  end
  os.execute("mkdir -p " .. q(path:match("^(.*)/")))
  hw.exec_async(curl(img, url, path), function(_, err)
    cb(err == nil, err)
  end)
end

function M.ensure_preview(img, cb)
  download(img, img.preview, M.preview_path(img), cb or function() end)
end

function M.fetch_image(img, cb)
  download(img, img.url, M.image_path(img), function(ok, err)
    if not ok then
      common.notify("Error fetching media", err or "download failed")
    end
    if cb then
      cb(ok)
    end
  end)
end

-- A copy of the downloaded image scaled to `width` * 2 px (decoding the
-- original, often 4K, would cost tens of MB for a 250 px widget).
-- `cb(path)` gets the scaled copy, else the best file available.
function M.display_file(img, width, cb)
  local full = M.image_path(img)
  if M.is_video(img) or M.is_zip(img) or not common.file_exists(full) then
    cb(common.file_exists(M.preview_path(img)) and M.preview_path(img) or nil)
    return
  end
  local w = math.floor(width * 2)
  local scaled = string.format("%s/%s/scaled/%s_%d.jpg", BOORU_DIR, api_value(img), img.id, w)
  if common.file_exists(scaled) then
    cb(scaled)
    return
  end
  os.execute("mkdir -p " .. q(scaled:match("^(.*)/")))
  hw.exec_async({ "magick", full .. "[0]", "-resize", w .. "x>", "-quality", "90", scaled }, function(_, err)
    cb(err and full or scaled)
  end)
end

function M.is_downloaded(img)
  return common.file_exists(M.image_path(img))
end

local function same(a, b)
  return a and b and tostring(a.id) == tostring(b.id) and api_value(a) == api_value(b)
end

function M.is_bookmarked(img)
  for _, b in ipairs(hw.settings:get("booru.bookmarks") or {}) do
    if same(b, img) then
      return true
    end
  end
  return false
end

function M.toggle_bookmark(img)
  local list, found = {}, false
  for _, b in ipairs(hw.settings:get("booru.bookmarks") or {}) do
    if same(b, img) then
      found = true
    else
      table.insert(list, b)
    end
  end
  if not found then
    table.insert(list, M.to_json(img))
  end
  hw.settings:set("booru.bookmarks", list)
  common.notify("Success", found and "Bookmark removed" or "Image bookmarked")
  return not found
end

function M.is_current_waifu(img)
  return same(hw.settings:get("waifuWidget.current"), img) == true
end

function M.set_as_current_waifu(img)
  hw.settings:set("waifuWidget.current", M.to_json(img))
end

local function pinned_path(img)
  local base = M.image_path(img):match("([^/]+)$"):gsub("%.[^.]+$", "")
  return require("lib.paths").fastfetch_cache .. "/" .. base .. ".webp"
end

function M.is_pinned(img)
  return common.file_exists(pinned_path(img))
end

-- Rounded copy for fastfetch (AGS pinToTerminal), toggled.
function M.pin_to_terminal(img, cb)
  if M.is_video(img) then
    common.notify("Error pinning to terminal", "Cannot pin videos to terminal")
    return
  end
  local target = pinned_path(img)
  local cmd
  if common.file_exists(target) then
    cmd = { "rm", "-f", target }
  else
    os.execute("mkdir -p " .. q(require("lib.paths").fastfetch_cache))
    local r = "%[fx:min(w,h)*0.05]"
    cmd = {
      "magick", M.image_path(img), "-alpha", "set", "(", "+clone", "-alpha", "transparent",
      "-background", "none", "-fill", "white", "-draw",
      "roundrectangle 0,0,%[fx:w-1],%[fx:h-1]," .. r .. "," .. r,
      ")", "-compose", "Dst_In", "-composite", "-strip", "-quality", "82",
      "-define", "webp:method=6", "-define", "webp:alpha-quality=90", "-background", "none", target,
    }
  end
  local unpin = cmd[1] == "rm"
  hw.exec_async(cmd, function(_, err)
    if err then
      common.notify("Error pinning to terminal", err)
      return
    end
    common.notify("Waifu", unpin and "UN-Pinned from Terminal" or "Pinned To Terminal")
    common.run({ "bash", "-c", "pkill -SIGUSR1 zsh || true" })
    if cb then
      cb(not unpin)
    end
  end)
end

function M.copy_to_clipboard(img)
  if M.is_video(img) then
    common.notify("Error copying to clipboard", "Cannot copy video to clipboard")
    return
  end
  hw.exec_async({ "bash", "-c", "wl-copy --type image/png < " .. q(M.image_path(img)) }, function(_, err)
    common.notify(err and "Error copying to clipboard" or "Success", err or "Image copied to clipboard")
  end)
end

local function exec_cmd(cmd)
  bar_common.dispatch(string.format("hl.dsp.exec_cmd(%q)", cmd))
end

function M.open_in_viewer(img)
  local path = q(M.image_path(img))
  if M.is_video(img) then
    exec_cmd("mpv --force-window=immediate --no-terminal --title=" .. q(img.id) .. " " .. path)
  else
    exec_cmd("swayimg -v " .. path)
  end
end

function M.open_in_browser(img)
  local url = (img.api and img.api.idSearchUrl or "") .. tostring(img.id)
  hw.exec_async({ "xdg-open", url }, function(_, err)
    common.notify(err and "Error opening in browser" or "Success", err or ("Opened " .. url))
  end)
end

function M.add_to_wallpapers(img)
  local dir = require("lib.paths").wallpapers .. "/custom"
  os.execute("mkdir -p " .. q(dir))
  hw.exec_async({ "cp", M.image_path(img), dir .. "/" .. img.id .. "." .. (img.extension or "jpg") }, function(_, err)
    common.notify(err and "Error adding to wallpapers" or "Success", err or "Media added to wallpapers")
  end)
end

local function file_size(path)
  local f = io.open(path, "rb")
  if not f then
    return nil
  end
  local size = f:seek("end")
  f:close()
  return size
end

-- utils/profile-avatar.ts: copies the image to ~/.face.icon (the preview
-- when the original is over 500 KB). `progress(status, text)` optional.
function M.set_profile_avatar(path, thumbnail, progress)
  progress = progress or function() end
  local size, tsize = file_size(path), thumbnail and file_size(thumbnail)
  if size and size > MAX_AVATAR_SIZE then
    if not tsize then
      common.notify("Avatar too large", string.format("Image exceeds %.1f KB", MAX_AVATAR_SIZE / 1024))
      return
    end
    path = thumbnail
  end
  local ext = (path:match("%.([^./]+)$") or ""):lower()
  if not ({ png = true, jpg = true, jpeg = true, webp = true })[ext] then
    common.notify("Invalid image", "Pick a PNG, JPG, or WebP file.")
    return
  end
  progress("loading", "Updating local avatar...")
  hw.exec_async({ "cp", path, HOME .. "/.face.icon" }, function(_, err)
    if err then
      progress("error", "Local update failed")
      common.notify("Local update failed", err)
      return
    end
    progress("success", "Avatar updated locally")
    common.notify("Avatar updated", "Using the selected picture locally.")
    M.avatar_changed:set(M.avatar_changed:peek() + 1)
  end)
end
M.avatar_changed = hw.state(0)

function M.to_json(img)
  return {
    id = tostring(img.id),
    width = tonumber(img.width) or 0,
    height = tonumber(img.height) or 0,
    api = img.api,
    tags = img.tags or {},
    extension = img.extension,
    url = img.url,
    preview = img.preview,
  }
end

local function credentials(api)
  local user = hw.settings:get("apiKeys." .. api .. ".user")
  local key = hw.settings:get("apiKeys." .. api .. ".key")
  if type(user) == "string" and user ~= "" and type(key) == "string" and key ~= "" then
    return user, key
  end
end

-- booru.py output: a JSON list, or {error = true, message = …} (stderr).
local function run_script(args, cb)
  local cmd = { "python", SCRIPT }
  for _, a in ipairs(args) do
    table.insert(cmd, a)
  end
  hw.exec_async(cmd, function(out, err)
    local ok, parsed = pcall(hw.json.decode, out or err or "")
    if ok and type(parsed) == "table" and parsed.error then
      cb(nil, parsed.message or "Unknown booru error")
    elseif err then
      local okm, e = pcall(hw.json.decode, err)
      cb(nil, okm and type(e) == "table" and e.message or err)
    elseif not ok or type(parsed) ~= "table" then
      cb(nil, "Invalid response from booru script")
    else
      cb(parsed)
    end
  end)
end

-- The stored API, or the first supported one if it was removed (old
-- settings may still name Danbooru/Safebooru).
local function current_api()
  local stored = hw.settings:get("booru.api")
  for _, api in ipairs(defaults.BOORU_APIS) do
    if stored and stored.value == api.value then
      return api
    end
  end
  return defaults.BOORU_APIS[1]
end

local function api_args(api, args)
  local user, key = credentials(api)
  if user then
    for _, a in ipairs({ "--api-user", user, "--api-key", key }) do
      table.insert(args, a)
    end
  end
  return args
end

-- Image dialog (popover of a grid item) ----------------------------------

local function Tags(img, limit)
  local box = { class = "tags", row_spacing = 5, column_spacing = 5 }
  for i, tag in ipairs(img.tags or {}) do
    if limit and i > limit then
      break
    end
    table.insert(
      box,
      W.Button({
        class = "tag",
        tooltip = tag,
        on_click = function()
          common.run({ "wl-copy", tag }, "Error")
        end,
        W.Label({ ellipsize = "end", max_width_chars = 10, label = tag }),
      })
    )
  end
  return W.FlowBox(box)
end

local function Dialog(img)
  local w, h = 300, 300
  local ratio = (tonumber(img.width) or 0) > 0 and (tonumber(img.height) or 0) > 0 and img.width / img.height or 1
  local dw = ratio >= 1 and w * ratio or w
  local dh = ratio >= 1 and h or h / ratio
  local downloaded = hw.state(M.is_downloaded(img))
  local bookmarked = hw.state(M.is_bookmarked(img))
  local pinned = hw.state(M.is_pinned(img))
  local still = not M.is_video(img) and not M.is_zip(img)

  local function need(text)
    return downloaded:map(function(d)
      return d and text or "<b>Download</b> first to " .. text:lower()
    end)
  end

  local media
  if M.is_zip(img) then
    media = Box({
      class = "media zip-placeholder",
      width_request = dw,
      height_request = dh,
      vertical = true,
      Box({ vexpand = true }),
      W.Label({ class = "zip-icon", label = "󰗄", halign = "center" }),
      W.Label({ class = "zip-label", label = "This type of video file cannot be played.", wrap = true, justify = "center" }),
      W.Label({ class = "zip-hint", label = "Open in browser to view media.", wrap = true, justify = "center" }),
      Box({ vexpand = true }),
    })
  else
    media = common.Picture({
      class = "image",
      width = dw,
      height = dh,
      file = downloaded:map(function(d)
        return (d and still) and M.image_path(img) or M.preview_path(img)
      end),
    })
  end

  local actions = Box({
    class = "actions",
    spacing = 10,
    vertical = true,
    Box({
      class = "section",
      W.Button({
        label = "\u{f08e}",
        tooltip = "Open in browser",
        hexpand = true,
        on_click = function()
          M.open_in_browser(img)
        end,
      }),
      common.toggle({
        class = "button",
        hexpand = true,
        tooltip = "Bookmark media",
        active = bookmarked,
        label = bookmarked:map(function(b)
          return b and "󰧌" or "\u{f02e}"
        end),
        on_toggle = function()
          bookmarked:set(M.toggle_bookmark(img))
        end,
      }),
    }),
    Box({
      class = "section",
      W.Button({
        label = "\u{f019}",
        tooltip = "Download media",
        hexpand = true,
        sensitive = downloaded:map(function(d)
          return not d
        end),
        on_click = function()
          M.fetch_image(img, function(ok)
            downloaded:set(ok and M.is_downloaded(img))
          end)
        end,
      }),
      W.Button({
        label = "\u{f0c5}",
        hexpand = true,
        tooltip_markup = need(still and "Copy image" or "Cannot copy video"),
        sensitive = downloaded:map(function(d)
          return d and still
        end),
        on_click = function()
          M.copy_to_clipboard(img)
        end,
      }),
      W.Button({
        label = "\u{f004}",
        hexpand = true,
        tooltip_markup = need("Set as current waifu"),
        sensitive = downloaded:map(function(d)
          return d and not M.is_video(img)
        end),
        on_click = function()
          M.set_as_current_waifu(img)
        end,
      }),
      W.Button({
        label = "\u{f007}",
        hexpand = true,
        tooltip_markup = need("Set as profile avatar"),
        sensitive = downloaded:map(function(d)
          return d and still
        end),
        on_click = function()
          M.set_profile_avatar(M.image_path(img), M.preview_path(img))
        end,
      }),
      W.Button({
        label = "\u{f03e}",
        hexpand = true,
        tooltip_markup = need("Open in viewer"),
        sensitive = downloaded,
        on_click = function()
          M.open_in_viewer(img)
        end,
      }),
      common.toggle({
        hexpand = true,
        active = pinned,
        label = pinned:map(function(p)
          return p and "󰐃" or "\u{f435}"
        end),
        tooltip_markup = need("Pin to terminal"),
        sensitive = downloaded:map(function(d)
          return d and not M.is_video(img)
        end),
        on_toggle = function()
          M.pin_to_terminal(img, function(p)
            pinned:set(p)
          end)
        end,
      }),
      W.Button({
        label = "󰸉",
        hexpand = true,
        tooltip_markup = need("Add to wallpapers"),
        sensitive = downloaded,
        on_click = function()
          M.add_to_wallpapers(img)
        end,
      }),
    }),
  })

  return W.Overlay({
    class = "booru-image",
    width_request = dw,
    height_request = dh,
    child = media,
    Box({
      vertical = true,
      width_request = dw,
      height_request = dh,
      Tags(img, 10),
      Box({ vexpand = true }),
      actions,
    }),
  })
end

M.Dialog = Dialog

-- A grid thumbnail; the dialog is built the first time it opens.
local function ImageButton(img, column_width, on_active)
  M.ensure_preview(img)
  local info = {}
  if M.is_pinned(img) then
    table.insert(info, "\u{f435}")
  end
  if M.is_bookmarked(img) then
    table.insert(info, "\u{f02e}")
  end
  if M.is_current_waifu(img) then
    table.insert(info, "\u{f004}")
  end
  local animated = M.is_video(img)
  for _, t in ipairs(img.tags or {}) do
    animated = animated or t == "animated"
  end
  if animated then
    table.insert(info, "\u{f04b}")
  end
  local opened = hw.state(false)
  local ratio = (tonumber(img.width) or 0) > 0 and img.height / img.width or 1
  return W.MenuButton({
    class = "image-button",
    hexpand = true,
    width_request = math.floor(column_width),
    height_request = math.floor(column_width * ratio),
    direction = "right",
    tooltip_markup = string.format(
      "Click to Open\nRight Click to Open in Browser\n<b>ID:</b> %s\n<b>Dimensions:</b> %sx%s",
      img.id,
      img.width,
      img.height
    ),
    on_click = function(_, button)
      if button == 3 then
        M.open_in_browser(img)
      end
    end,
    child = common.Picture({ file = M.preview_path(img), class = "image", info = info, hexpand = true, vexpand = true }),
    popover = W.Popover({
      class = "popover-open",
      on_change = function(_, visible)
        if visible then
          opened:set(true)
        end
        if on_active then
          on_active(visible)
        end
      end,
      hw.With(opened, function(o)
        return o and Dialog(img) or Box({})
      end),
    }),
  })
end

local function TagButton(tag, class, on_click)
  return W.Button({
    class = class,
    tooltip = tag,
    on_click = on_click,
    W.Label({ ellipsize = "end", max_width_chars = 10, label = tag }),
  })
end

-- Shortest-column-first layout (AGS masonry).
local function masonry(images, cols)
  local columns = {}
  for i = 1, cols do
    columns[i] = { height = 0, items = {} }
  end
  for _, img in ipairs(images) do
    local target = columns[1]
    for _, c in ipairs(columns) do
      if c.height < target.height then
        target = c
      end
    end
    table.insert(target.items, img)
    local w, h = tonumber(img.width) or 0, tonumber(img.height) or 0
    target.height = target.height + ((w > 0 and h > 0) and h / w or 1)
  end
  return columns
end

-- BooruViewer ---------------------------------------------------------------

local images = hw.state({})
local fetching = false

local function ensure_rating_first()
  local tags = hw.settings:get("booru.tags") or {}
  local rating, rest = nil, {}
  for _, t in ipairs(tags) do
    if t:match("^" .. RATING .. "$") then
      rating = rating or t
    else
      table.insert(rest, t)
    end
  end
  table.insert(rest, 1, rating or "-rating:explicit")
  hw.settings:set("booru.tags", rest)
end

hw.define("BooruViewer", function(props)
  local cache_size = hw.state("0mb")
  local status = hw.state("idle")
  local fetched_tags = hw.state({})
  local stored_tab = hw.settings:get("booru.selectedTab")
  local selected = hw.state(stored_tab == "Bookmarks" and stored_tab or current_api().name)
  local bottom_revealed = hw.state(false)
  local page = hw.state(1)
  local direction = hw.state("slide_left")
  local subs = {}

  local function active()
    local w = hw.settings:get("leftPanel.widget")
    return w and w.name == "BooruViewer"
  end

  local function calc_cache_size()
    local api = current_api().value
    hw.exec_async({ "du", "-sb", BOORU_DIR .. "/" .. api .. "/previews" }, function(out)
      local bytes = tonumber((out or ""):match("^(%d+)")) or 0
      cache_size:set(string.format("%dmb", math.floor(bytes / (1024 * 1024) + 0.5)))
    end)
  end

  local function fetch_images()
    if not active() or fetching then
      return
    end
    fetching = true
    status:set("loading")
    local limit = hw.settings:get("booru.limit") or 12
    local current = math.max(1, page:peek())
    local function done(list, err)
      fetching = false
      if not list then
        local bm = selected:peek() == "Bookmarks"
        common.notify(bm and "Error loading bookmarks" or "Error fetching images", err or "failed")
        status:set("error")
        return
      end
      -- Thumbnails must exist before the pictures are created.
      local pending = #list + 1
      local function one()
        pending = pending - 1
        if pending == 0 then
          images:set(list)
          calc_cache_size()
          status:set("success")
        end
      end
      for _, img in ipairs(list) do
        M.ensure_preview(img, one)
      end
      one()
    end
    if selected:peek() == "Bookmarks" then
      local all = hw.settings:get("booru.bookmarks") or {}
      local list = {}
      local first = limit > 0 and (current - 1) * limit + 1 or 1
      local last = limit > 0 and first + limit - 1 or #all
      for i = first, math.min(last, #all) do
        table.insert(list, all[i])
      end
      done(list)
    else
      local api = current_api()
      local args = api_args(api.value, {
        "--api", api.value,
        "--tags", table.concat(hw.settings:get("booru.tags") or {}, ","),
        "--limit", tostring(limit),
        "--page", tostring(current),
      })
      run_script(args, function(list, err)
        if list then
          for _, img in ipairs(list) do
            img.api = api
          end
        end
        done(list, err)
      end)
    end
  end

  local function fetch_tags(tag)
    local api = current_api().value
    run_script(api_args(api, { "--api", api, "--tag", tag }), function(list, err)
      if not list then
        common.notify("Error fetching tags", err)
        fetched_tags:set({})
        return
      end
      fetched_tags:set(list)
    end)
  end

  local function go(n, dir)
    if n < 1 then
      return
    end
    direction:set(dir == "prev" and "slide_right" or "slide_left")
    page:set(n)
  end

  local function add_tags(list)
    local tags, seen = {}, {}
    for _, t in ipairs(hw.settings:get("booru.tags") or {}) do
      if not seen[t] then
        seen[t] = true
        table.insert(tags, t)
      end
    end
    for _, t in ipairs(list) do
      if not seen[t] then
        seen[t] = true
        table.insert(tags, t)
      end
    end
    hw.settings:set("booru.tags", tags)
  end

  local stack_page = hw.state("a")
  local page_a, page_b = hw.state(images:peek()), hw.state({})
  local function images_page(list)
    local cols = masonry(list, hw.settings:get("booru.columns") or 2)
    local column_width = (hw.settings:get("leftPanel.width") or 400) / #cols - 10
    local row = { class = "images", spacing = 5 }
    for _, col in ipairs(cols) do
      local c = { vertical = true, spacing = 5, hexpand = true }
      for _, img in ipairs(col.items) do
        table.insert(c, ImageButton(img, column_width, props.on_active))
      end
      table.insert(row, Box(c))
    end
    return W.Scrolled({ hexpand = true, vexpand = true, hscroll = "never", Box(row) })
  end

  local function slider(label, value, set, fmt, steps)
    local timer
    return Box({
      class = "setting",
      spacing = 5,
      W.Label({ label = label, hexpand = true, xalign = 0 }),
      Box({
        spacing = 5,
        halign = "end",
        W.Slider({
          class = "slider",
          draw_value = false,
          hexpand = true,
          min = 0,
          max = 1,
          step = 1 / steps,
          width_request = hw.settings["leftPanel.width"]:map(function(wd)
            return math.floor((wd or 400) / 2)
          end),
          value = value,
          on_change = function(_, v)
            if timer then
              timer:cancel()
            end
            timer = hw.timeout(300, function()
              timer = nil
              set(v)
            end)
          end,
        }),
        W.Label({ label = value:map(fmt), width_request = 50 }),
      }),
    })
  end

  local tags_sig = hw.settings["booru.tags"]
  local entry_timer

  local root = Box({
    class = "booru",
    vertical = true,
    hexpand = true,
    spacing = 10,
    on_key = function(_, key)
      if key == "Up" then
        bottom_revealed:set(true)
      elseif key == "Down" then
        bottom_revealed:set(false)
      elseif key == "Right" then
        go(page:peek() + 1, "next")
      elseif key == "Left" and page:peek() > 1 then
        go(page:peek() - 1, "prev")
      else
        return false
      end
      return true
    end,
    on_destroy = function()
      for _, u in ipairs(subs) do
        u()
      end
    end,
    -- Two pages: a new result fills the hidden one and slides in.
    W.Stack({
      hexpand = true,
      vexpand = true,
      duration = common.TRANSITION,
      transition = direction,
      shown = stack_page,
      Box({ name = "a", hw.With(page_a, images_page) }),
      Box({ name = "b", hw.With(page_b, images_page) }),
    }),
    Box({
      class = "bottom-bar",
      vertical = true,
      spacing = 10,
      Box({
        class = "actions",
        vertical = true,
        Box({
          class = "navigation",
          spacing = 10,
          W.Button({
            label = "\u{f053}",
            tooltip = "KEY-LEFT",
            on_click = function()
              go(page:peek() - 1, "prev")
            end,
          }),
          W.Button({
            class = "reveal-button",
            hexpand = true,
            tooltip = "Toggle Settings (KEY-UP/DOWN)",
            label = bottom_revealed:map(function(v)
              return v and "\u{f107}" or "\u{f106}"
            end),
            on_click = function()
              bottom_revealed:set(not bottom_revealed:peek())
            end,
          }),
          W.Button({
            label = "\u{f054}",
            tooltip = "KEY-RIGHT",
            on_click = function()
              go(page:peek() + 1, "next")
            end,
          }),
        }),
        W.Revealer({
          class = "revealer",
          transition = "swing_up",
          duration = common.TRANSITION,
          reveal = bottom_revealed,
          Box({
            class = "options",
            vertical = true,
            spacing = 10,
            Box({
              class = "pages",
              spacing = 5,
              halign = "center",
              hw.With(page, function(current)
                local total = math.floor((hw.settings:get("leftPanel.width") or 400) / 100 + 2)
                local buttons = { spacing = 5 }
                if current > 6 then
                  table.insert(
                    buttons,
                    W.Button({
                      class = "first",
                      label = "1",
                      on_click = function()
                        go(1, "prev")
                      end,
                    })
                  )
                  table.insert(buttons, W.Label("..."))
                end
                local start = math.max(1, current - total // 2)
                for n = start, start + total - 1 do
                  table.insert(
                    buttons,
                    W.Button({
                      label = n ~= current and tostring(n) or "\u{f021}",
                      on_click = function()
                        if n ~= current then
                          go(n, n > current and "next" or "prev")
                        else
                          fetch_images()
                        end
                      end,
                    })
                  )
                end
                return Box(buttons)
              end),
            }),
            slider("Limit", hw.settings["booru.limit"]:map(function(l)
              return (l or 0) / 100
            end), function(v)
              hw.settings:set("booru.limit", math.floor(v * 100 + 0.5))
            end, function(v)
              return tostring(math.floor(v * 100 + 0.5))
            end, 10),
            slider("Columns", hw.settings["booru.columns"]:map(function(c)
              return ((c or 2) - 1) / 4
            end), function(v)
              hw.settings:set("booru.columns", math.floor(v * 4 + 0.5) + 1)
            end, function(v)
              return tostring(math.floor(v * 4 + 0.5) + 1)
            end, 4),
            Box({
              class = "input",
              spacing = 5,
              vertical = true,
              Box({
                class = "tags",
                vertical = true,
                spacing = 5,
                W.FlowBox({
                  column_spacing = 5,
                  row_spacing = 5,
                  children = fetched_tags:map(function(list)
                    local out = {}
                    for _, tag in ipairs(list or {}) do
                      table.insert(out, TagButton(tag, "tag fetched", function()
                        add_tags({ tag })
                      end))
                    end
                    return out
                  end),
                }),
                W.FlowBox({
                  column_spacing = 5,
                  row_spacing = 5,
                  children = tags_sig:map(function(list)
                    local out = {}
                    for _, tag in ipairs(list or {}) do
                      local rating = tag:match("^" .. RATING .. "$") ~= nil
                      table.insert(out, TagButton(tag, rating and "tag rating" or "tag enabled", function()
                        local rest = {}
                        for _, t in ipairs(hw.settings:get("booru.tags") or {}) do
                          if t ~= tag and not (rating and t:match("^" .. RATING .. "$")) then
                            table.insert(rest, t)
                          end
                        end
                        if rating then
                          table.insert(rest, 1, tag:sub(1, 1) == "-" and "rating:explicit" or "-rating:explicit")
                        end
                        hw.settings:set("booru.tags", rest)
                      end))
                    end
                    return out
                  end),
                }),
              }),
              Box({
                spacing = 5,
                W.Entry({
                  hexpand = true,
                  placeholder = "Add a Tag",
                  on_change = function(_, text)
                    if entry_timer then
                      entry_timer:cancel()
                    end
                    entry_timer = hw.timeout(200, function()
                      entry_timer = nil
                      if text == "" then
                        fetched_tags:set({})
                      else
                        fetch_tags(text)
                      end
                    end)
                  end,
                  on_activate = function(_, text)
                    local parts = {}
                    for p in text:gmatch("%S+") do
                      table.insert(parts, p)
                    end
                    if #parts > 0 then
                      add_tags(parts)
                    end
                  end,
                }),
                W.Button({
                  class = "clear",
                  halign = "center",
                  valign = "center",
                  label = cache_size,
                  tooltip = "Clear Cache",
                  on_click = function()
                    local base = BOORU_DIR .. "/" .. current_api().value
                    hw.exec_async({
                      "bash", "-c",
                      string.format("rm -rf %s %s && mkdir -p %s %s", q(base .. "/previews"), q(base .. "/images"), q(base .. "/previews"), q(base .. "/images")),
                    }, function(_, err)
                      common.notify(err and "Error clearing cache" or "Success", err or "Cache cleared successfully")
                      calc_cache_size()
                    end)
                  end,
                }),
              }),
            }),
          }),
        }),
      }),
      (function()
        local tabs = { class = "tab-list", spacing = 5 }
        for _, api in ipairs(defaults.BOORU_APIS) do
          table.insert(
            tabs,
            common.toggle({
              class = "api",
              hexpand = true,
              label = api.name,
              active = selected:map(function(t)
                return t == api.name
              end),
              on_toggle = function()
                hw.settings:set("booru.api", api)
                hw.settings:set("booru.selectedTab", api.name)
              end,
            })
          )
        end
        table.insert(
          tabs,
          common.toggle({
            class = "bookmarks",
            label = "\u{f02e}",
            active = selected:map(function(t)
              return t == "Bookmarks"
            end),
            on_toggle = function()
              page:set(1)
              hw.settings:set("booru.selectedTab", "Bookmarks")
            end,
          })
        )
        return Box(tabs)
      end)(),
    }),
    common.Progress({ status = status, transition = "swing_up", class = "booru-progress" }),
  })

  ensure_rating_first()
  local last_tags = table.concat(hw.settings:get("booru.tags") or {}, ",")
  local function watch(sig, fn)
    table.insert(subs, sig:subscribe(fn))
  end
  watch(page, fetch_images)
  watch(hw.settings["booru.limit"], fetch_images)
  watch(images, function(list)
    local next_page = stack_page:peek() == "a" and "b" or "a"
    ;(next_page == "a" and page_a or page_b):set(list)
    stack_page:set(next_page)
  end)
  watch(hw.settings["booru.columns"], function()
    images:set(images:peek())
  end)
  watch(tags_sig, function(tags)
    local joined = table.concat(tags or {}, ",")
    if joined ~= last_tags then
      last_tags = joined
      if selected:peek() ~= "Bookmarks" then
        fetch_images()
      end
    end
  end)
  watch(hw.settings["booru.selectedTab"], function(tab)
    if tab ~= selected:peek() then
      selected:set(tab)
      fetch_images()
    end
  end)
  -- First activation fetches (the panel builds every widget up front).
  watch(hw.settings["leftPanel.widget"], function()
    if active() and #images:peek() == 0 then
      fetch_images()
    end
  end)
  fetch_images()
  return root
end)

-- Waifu ---------------------------------------------------------------------

local function random_bookmark()
  local list = hw.settings:get("booru.bookmarks") or {}
  if #list == 0 then
    return nil
  end
  return list[math.random(#list)]
end

-- Random image from the bookmarks every `waifuWidget.random_interval` s.
local random_timer
local function update_interval()
  if random_timer then
    random_timer:cancel()
    random_timer = nil
  end
  local secs = tonumber(hw.settings:get("waifuWidget.random_interval")) or 0
  if secs > 0 then
    random_timer = hw.timeout(secs * 1000, function()
      random_timer = nil
      local img = random_bookmark()
      if img then
        M.set_as_current_waifu(img)
      end
      update_interval()
    end)
  end
end

local function WaifuImage(img, width)
  local status = hw.state("idle")
  -- Preview first, then the scaled download.
  local file = hw.state(common.file_exists(M.preview_path(img)) and M.preview_path(img) or nil)
  local function show_best()
    M.display_file(img, width, function(path)
      if path then
        file:set(path)
      end
    end)
  end
  M.ensure_preview(img, function(ok)
    if ok and not file:peek() then
      file:set(M.preview_path(img))
    end
    if M.is_downloaded(img) then
      show_best()
    else
      M.fetch_image(img, function(done)
        if done then
          show_best()
        end
      end)
    end
  end)
  local bookmarked = hw.state(M.is_bookmarked(img))
  local pinned = hw.state(M.is_pinned(img))
  local search_api = img.api or defaults.BOORU_APIS[1]
  local w, h = tonumber(img.width) or 0, tonumber(img.height) or 0
  local height = (w > 0 and h > 0) and h / w * width or width
  local entry

  local function fetch_by_id(id)
    status:set("loading")
    local api = search_api
    run_script(api_args(api.value, { "--api", api.value, "--id", id }), function(list, err)
      if not list or not list[1] then
        status:set("error")
        common.notify("Error", err or "Post not found")
        return
      end
      local found = list[1]
      found.api = api
      M.fetch_image(found, function(ok)
        status:set(ok and "success" or "error")
        if ok then
          hw.settings:set("waifuWidget.input_history", id)
          M.set_as_current_waifu(found)
        end
      end)
    end)
  end

  local function upload()
    hw.exec_async({
      "zenity", "--file-selection", "--title=Select Image",
      "--file-filter=Images (png, jpg, webp, gif, mp4, webm) | *.png *.jpg *.jpeg *.webp *.gif *.mp4 *.webm",
    }, function(out)
      local path = (out or ""):match("^%s*(.-)%s*$")
      if path == "" then
        return
      end
      local ext = path:match("%.([^./]+)$") or "jpg"
      local id = tostring(os.time())
      local custom = { id = id, api = { name = "Custom", value = "custom" }, extension = ext, tags = { "custom" } }
      local dst, prev = M.image_path(custom), M.preview_path(custom)
      local probe = M.is_video(custom)
          and "ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0:s=x " .. q(path)
        or "identify -format '%wx%h' " .. q(path .. "[0]")
      hw.exec_async({
        "bash", "-c",
        string.format("mkdir -p %s %s && cp -- %s %s && ln -sf %s %s && %s",
          q(dst:match("^(.*)/")), q(prev:match("^(.*)/")), q(path), q(dst), q(dst), q(prev), probe),
      }, function(dims, err)
        if err then
          common.notify("Error", err)
          return
        end
        local cw, ch = (dims or ""):match("(%d+)x(%d+)")
        custom.width, custom.height = tonumber(cw) or 0, tonumber(ch) or 0
        M.set_as_current_waifu(custom)
        common.notify("Waifu", "Custom image set")
      end)
    end)
  end

  local api_buttons = { class = "section", spacing = 5 }
  local api_state = hw.state(search_api.value)
  for _, api in ipairs(defaults.BOORU_APIS) do
    table.insert(
      api_buttons,
      common.toggle({
        class = "api",
        hexpand = true,
        label = api.name,
        active = api_state:map(function(v)
          return v == api.value
        end),
        on_toggle = function()
          search_api = api
          api_state:set(api.value)
        end,
      })
    )
  end

  local media
  if M.is_zip(img) then
    media = Box({
      class = "image zip-placeholder",
      width_request = width,
      height_request = height,
      vertical = true,
      Box({ vexpand = true }),
      W.Label({ class = "zip-icon", label = "󰗄", halign = "center" }),
      W.Label({ class = "zip-label", label = "This type of video file cannot be played.", wrap = true, justify = "center" }),
      Box({ vexpand = true }),
    })
  else
    media = common.Picture({ class = "image", width = width, height = height, file = file })
  end

  return W.Overlay({
    class = "booru-image",
    child = media,
    Box({
      class = "actions",
      valign = "end",
      vertical = true,
      spacing = 5,
      common.Progress({ status = status }),
      Box({
        class = "section",
        common.toggle({
          class = "button",
          hexpand = true,
          tooltip = "Bookmark media",
          active = bookmarked,
          label = bookmarked:map(function(b)
            return b and "󰧌" or "\u{f02e}"
          end),
          on_toggle = function()
            bookmarked:set(M.toggle_bookmark(img))
          end,
        }),
        common.toggle({
          class = "pin",
          hexpand = true,
          sensitive = not M.is_video(img),
          active = pinned,
          label = pinned:map(function(p)
            return p and "󰐃" or "\u{f435}"
          end),
          tooltip = pinned:map(function(p)
            return M.is_video(img) and "Cannot pin videos" or p and "Unpin media from terminal" or "Pin media to terminal"
          end),
          on_toggle = function()
            M.pin_to_terminal(img, function(p)
              pinned:set(p)
            end)
          end,
        }),
      }),
      Box({
        class = "section",
        W.Button({
          label = "\u{f03e}",
          class = "open",
          hexpand = true,
          on_click = function()
            M.open_in_viewer(img)
          end,
        }),
        W.Button({
          label = "\u{f08e}",
          class = "browser",
          hexpand = true,
          on_click = function()
            M.open_in_browser(img)
          end,
        }),
        W.Button({
          label = "\u{f0c5}",
          class = "copy",
          hexpand = true,
          sensitive = not M.is_video(img),
          on_click = function()
            M.copy_to_clipboard(img)
          end,
        }),
        W.Button({
          label = "󰸉",
          class = "wallpaper",
          hexpand = true,
          tooltip = "Add to wallpapers",
          on_click = function()
            M.add_to_wallpapers(img)
          end,
        }),
      }),
      Box({
        class = "section",
        spacing = 5,
        W.Button({
          label = "\u{f002}",
          class = "entry-search",
          hexpand = true,
          on_click = function()
            if entry then
              fetch_by_id(entry:get("text"))
            end
          end,
        }),
        W.Entry({
          class = "input",
          placeholder = "enter post ID",
          text = hw.settings:get("waifuWidget.input_history") or "",
          setup = function(self)
            entry = self
          end,
          on_activate = function(_, text)
            fetch_by_id(text)
          end,
        }),
        W.Button({ label = "\u{f40a}", class = "upload", hexpand = true, on_click = upload }),
      }),
      Box(api_buttons),
    }),
  })
end

local randomized = false

hw.define("Waifu", function(props)
  -- AGS shows a random bookmark when the widget first appears.
  if not randomized then
    randomized = true
    local img = random_bookmark()
    if img then
      M.set_as_current_waifu(img)
    end
    hw.settings["waifuWidget.random_interval"]:subscribe(update_interval)
    update_interval()
  end
  local width = hw.settings["rightPanel.width"]
  local current = hw.settings["waifuWidget.current"]
  local key = hw.computed(function()
    local c, wd = current(), width()
    return { img = c, width = wd }
  end)
  return Box({
    class = common.class("waifu", props.class),
    vertical = true,
    css = "border-radius: 10px;",
    hw.With(key, function(k)
      local img = k.img
      if type(img) ~= "table" or not img.id then
        return W.Button({
          on_click = function()
            local left = props.monitor and hw.window.get("left-panel-" .. props.monitor)
            if left then
              left:show()
            end
            hw.settings:set("leftPanel.widget", defaults.LEFT_WIDGETS[2])
          end,
          Box({
            class = "no-image",
            halign = "center",
            valign = "center",
            vertical = true,
            W.Label({ label = "<b>No image selected</b>", use_markup = true }),
            W.Label("Open Booru Viewer"),
            W.Label("Select an image"),
            W.Label("Download then Waifu it"),
          }),
        })
      end
      return WaifuImage(img, (k.width or 250) - 10)
    end),
  })
end)

return M
