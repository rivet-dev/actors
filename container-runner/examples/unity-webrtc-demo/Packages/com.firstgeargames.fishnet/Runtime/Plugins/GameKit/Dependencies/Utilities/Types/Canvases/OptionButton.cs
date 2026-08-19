#if TEXTMESHPRO
using TMPro;
using UnityEngine;

namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class OptionMenuButton : MonoBehaviour
    {
        #region Public.
        /// <summary>
        /// ButtonData for this button.
        /// </summary>
        public ButtonData ButtonData { get; protected set; }
        #endregion

        #region Serialized.
        /// <summary>
        /// Text component to show button text.
        /// </summary>
        [Tooltip("Text component to show button text.")]
        [SerializeField]
        private TextMeshProUGUI _text;
        #endregion

        public virtual void Initialize(ButtonData buttonData)
        {
            ButtonData = buttonData;
            _text.text = buttonData.Text;
        }
    }
}
#endif