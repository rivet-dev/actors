using System;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class ImageButtonData : ButtonData
    {
        #region Public.
        /// <summary>
        /// Image to display.
        /// </summary>
        public Sprite DisplayImage { get; protected set; } = null;
        #endregion

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        /// <param name = "sprite">Image to use on the button.</param>
        /// <param name = "text">Text to display on the button.</param>
        /// <param name = "callback">Callback when OnPressed is called.</param>
        /// <param name = "key">Optional key to include within the callback.</param>
        public void Initialize(Sprite sprite, string text, PressedDelegate callback, string key = "")
        {
            base.Initialize(text, callback, key);
            DisplayImage = sprite;
        }

        public override void ResetState()
        {
            base.ResetState();
            DisplayImage = null;
        }
    }
}