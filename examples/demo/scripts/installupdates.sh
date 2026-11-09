#!/usr/bin/env bash
#    ____         __       ____               __     __
#   /  _/__  ___ / /____ _/ / / __ _____  ___/ /__ _/ /____ ___
#  _/ // _ \(_-</ __/ _ `/ / / / // / _ \/ _  / _ `/ __/ -_|_-<
# /___/_//_/___/\__/\_,_/_/_/  \_,_/ .__/\_,_/\_,_/\__/\__/___/
#                                 /_/
#

# Check if command exists
_checkCommandExists() {
  cmd="$1"
  if ! command -v "$cmd" >/dev/null; then
    echo 1
    return
  fi
  echo 0
  return
}

_isInstalled() {
  package="$1"
  case $install_platform in
  arch)
    check="$($aur_helper -Qs --color always "${package}" | grep "local" | grep "${package} ")"
    ;;
  fedora)
    check="$(dnf repoquery --quiet --installed ""${package}*"")"
    ;;
  *) ;;
  esac

  if [ -n "${check}" ]; then
    echo 0 #'0' means 'true' in Bash
    return #true
  fi
  echo 1 #'1' means 'false' in Bash
  return #false
}

# ------------------------------------------------------
# Confirm Start
# ------------------------------------------------------

sleep 1
clear
echo "================================"
echo "       UPDATES PACKAGES"
echo "================================"
echo
read -p "DO YOU WANT TO START THE UPDATE NOW? (Y/n): " response
echo
# Default to Yes if empty, or check if user typed Y/y, otherwise cancel
if [[ -z "$response" ]] || [[ "$response" =~ ^[Yy]$ ]]; then
  echo
  echo ":: Update starting..."
else
  echo
  echo ":: Update canceled."
  exit
fi

# -----------------------------------------------------
# Update mirrorlist
# -----------------------------------------------------
# sudo echo "Updating mirrorlist..."
# tmp=$(mktemp) && reflector --country Vietnam,China,Hongkong,Singapore --threads 4 --sort rate --save "$tmp" && rankmirrors "$tmp" | sudo tee /etc/pacman.d/mirrorlist >/dev/null
# rm -f "$tmp"

# -----------------------------------------------------
# Install update
# -----------------------------------------------------

# Arch
if [[ $(_checkCommandExists "pacman") == 0 ]]; then

  yay_installed="false"
  paru_installed="false"
  if [[ $(_checkCommandExists "yay") == 0 ]]; then
    yay_installed="true"
  fi
  if [[ $(_checkCommandExists "paru") == 0 ]]; then
    paru_installed="true"
  fi
  if [[ $yay_installed == "true" ]] && [[ $paru_installed == "false" ]]; then
    yay
  elif [[ $yay_installed == "false" ]] && [[ $paru_installed == "true" ]]; then
    paru -Syu --noconfirm
  else
    yay
  fi

# Fedora
elif [[ $(_checkCommandExists "dnf") == 0 ]]; then
  sudo dnf upgrade
else
  echo ":: ERROR - Platform not supported"
  echo "Press [ENTER] to close."
  read
fi
echo

# Flatpak
# echo ":: Searching for Flatpak updates..."
# flatpak update
# echo

# Reload Waybar
pkill -RTMIN+1 waybar

# Finishing
echo ":: Update complete! Press [ENTER] to close."
read
