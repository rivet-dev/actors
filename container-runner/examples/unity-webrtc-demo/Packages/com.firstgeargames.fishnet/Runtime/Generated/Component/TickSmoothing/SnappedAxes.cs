using System;
using GameKit.Dependencies.Utilities;

namespace FishNet.Component.Transforming
{
    /// <summary>
    /// Axes to snap of properties.
    /// </summary>
    [Flags]
    public enum SnappedAxes : uint
    {
        Unset = 0,
        X = 1 << 0,
        Y = 1 << 1,
        Z = 1 << 2,
        Everything = Enums.SHIFT_EVERYTHING_UINT
    }

    public static class SnappedAxesExtensions
    {
        public static bool FastContains(this SnappedAxes whole, SnappedAxes part) => (whole & part) == part;
    }
}