using GameKit.Dependencies.Utilities;

namespace FishNet.Object
{
    [System.Flags]
    public enum TransformPropertiesFlag : uint
    {
        Unset = 0,
        Position = 1 << 0,
        Rotation = 1 << 1,
        Scale = 1 << 2,
        Everything = Enums.SHIFT_EVERYTHING_UINT
    }

    public static class TransformPropertiesOptionExtensions
    {
        /// <summary>
        /// Returns if enum contains a value.
        /// </summary>
        /// <param name = "whole">Value checked against.</param>
        /// <param name = "part">Value checked if whole contains.</param>
        /// <returns></returns>
        public static bool FastContains(this TransformPropertiesFlag whole, TransformPropertiesFlag part) => (whole & part) == part;
    }
}