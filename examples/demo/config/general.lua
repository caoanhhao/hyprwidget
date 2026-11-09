hw.config({
  general = {
    -- "inherit" (the default) keeps whatever Hyprland is using.
    gaps_out = "inherit",
    -- A function computes the value from Hyprland's: h.general.gaps_in, …
    gaps_in = function(h)
      return math.max(4, tonumber(h.general.gaps_in) or 4)
    end,
  },
})
