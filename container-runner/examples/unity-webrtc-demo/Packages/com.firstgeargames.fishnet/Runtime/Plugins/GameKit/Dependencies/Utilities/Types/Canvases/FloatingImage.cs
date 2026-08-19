using Sirenix.OdinInspector;
using UnityEngine;
using UnityEngine.UI;

namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class FloatingImage : FloatingContainer
    {
        /// <summary>
        /// Renderer to apply sprite on.
        /// </summary>
        [Tooltip("Renderer to apply sprite on.")]
        [SerializeField]
        [TabGroup("Components")]
        protected Image Renderer;

        /// <summary>
        /// Sets which sprite to use.
        /// </summary>
        /// <param name = "sprite">Sprite to use.</param>
        /// <param name = "sizeOverride">When has value the renderer will be set to this size. Otherwise, the size of the sprite will be used. This value assumes the sprite anchors are set to center.</param>
        public virtual void SetSprite(Sprite sprite, Vector3? sizeOverride)
        {
            Renderer.sprite = sprite;
            Vector3 size = sizeOverride == null ? sprite.bounds.size * sprite.pixelsPerUnit : sizeOverride.Value;

            Renderer.rectTransform.sizeDelta = size;
        }
    }
}