using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    public static class Materials
    {
        /// <summary>
        /// Returns the color or tint color property for a material.
        /// </summary>
        /// <param name = "material"></param>
        /// <returns></returns>
        public static Color GetColor(this Material material)
        {
            if (material.HasProperty("_Color"))
                return material.color;
            else if (material.HasProperty("_TintColor"))
                return material.GetColor("_TintColor");

            return Color.white;
        }

        /// <summary>
        /// Sets the color or tint color property for a material.
        /// </summary>
        /// <param name = "material"></param>
        public static void SetColor(this Material material, Color color)
        {
            if (material.HasProperty("_Color"))
                material.color = color;
            else if (material.HasProperty("_TintColor"))
                material.SetColor("_TintColor", color);
        }
    }
}