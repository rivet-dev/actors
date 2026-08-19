using UnityEngine;

namespace GameKit.Dependencies.Utilities
{
    /// <summary>
    /// Ways a CanvasGroup can have it's blocking properties modified.
    /// </summary>
    public enum CanvasGroupBlockingType
    {
        Unchanged = 0,
        DoNotBlock = 1,
        Block = 2
    }

    public static class CanvaseGroups
    {
        public static void SetBlockingType(this CanvasGroup group, CanvasGroupBlockingType blockingType)
        {
            if (blockingType == CanvasGroupBlockingType.Unchanged)
                return;

            bool block = blockingType == CanvasGroupBlockingType.Block;
            group.blocksRaycasts = block;
            group.interactable = block;
        }

        /// <summary>
        /// Sets a CanvasGroup blocking type and alpha.
        /// </summary>
        /// <param name = "blockingType">How to handle interactions.</param>
        /// <param name = "alpha">Alpha for CanvasGroup.</param>
        public static void SetActive(this CanvasGroup group, CanvasGroupBlockingType blockingType, float alpha)
        {
            group.SetBlockingType(blockingType);
            group.alpha = alpha;
        }

        /// <summary>
        /// Sets a canvasGroup active with specified alpha.
        /// </summary>
        public static void SetActive(this CanvasGroup group, float alpha)
        {
            group.SetActive(true, false);
            group.alpha = alpha;
        }

        /// <summary>
        /// Sets a canvasGroup inactive with specified alpha.
        /// </summary>
        public static void SetInactive(this CanvasGroup group, float alpha)
        {
            group.SetActive(false, false);
            group.alpha = alpha;
        }

        /// <summary>
        /// Sets a group active state by changing alpha and interaction toggles.
        /// </summary>
        public static void SetActive(this CanvasGroup group, bool active, bool setAlpha)
        {
            if (group == null)
                return;

            if (setAlpha)
            {
                if (active)
                    group.alpha = 1f;
                else
                    group.alpha = 0f;
            }

            group.interactable = active;
            group.blocksRaycasts = active;
        }

        /// <summary>
        /// Sets a group active state by changing alpha and interaction toggles with a custom alpha.
        /// </summary>
        public static void SetActive(this CanvasGroup group, bool active, float alpha)
        {
            if (group == null)
                return;

            group.alpha = alpha;

            group.interactable = active;
            group.blocksRaycasts = active;
        }
    }
}