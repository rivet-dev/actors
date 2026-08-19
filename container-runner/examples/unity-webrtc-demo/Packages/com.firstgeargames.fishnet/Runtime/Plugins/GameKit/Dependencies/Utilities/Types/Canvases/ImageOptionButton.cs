#if TEXTMESHPRO
using TMPro;
using UnityEngine;
using UnityEngine.UI;

namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class OptionMenuImageButton : OptionMenuButton
    {
        #region Serialized.
        /// <summary>
        /// Image component to show image on.
        /// </summary>
        [Tooltip("Image component to show image on.")]
        [SerializeField]
        private Image _image;
        #endregion

        public virtual void Initialize(ImageButtonData buttonData)
        {
            base.Initialize(buttonData);
            _image.sprite = buttonData.DisplayImage;
        }
    }
}
#endif