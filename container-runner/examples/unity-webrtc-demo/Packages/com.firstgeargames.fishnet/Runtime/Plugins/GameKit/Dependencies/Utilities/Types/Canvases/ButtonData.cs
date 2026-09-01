namespace GameKit.Dependencies.Utilities.Types.CanvasContainers
{
    public class ButtonData : IResettable
    {
        #region Public.
        /// <summary>
        /// Text to place on the button.
        /// </summary>
        public string Text { get; protected set; } = string.Empty;

        /// <summary>
        /// When not null this will be called when action is taken.
        /// </summary>
        /// <param name = "key">Optional key to associate with callback.</param>
        public delegate void PressedDelegate(string key);

        /// <summary>
        /// Optional key to include within the callback.
        /// </summary>
        public string Key { get; protected set; } = string.Empty;
        #endregion

        /// <summary>
        /// Delegate to invoke when pressed.
        /// </summary>
        private PressedDelegate _delegate = null;

        /// <summary>
        /// Initializes this for use.
        /// </summary>
        /// <param name = "text">Text to display on the button.</param>
        /// <param name = "callback">Callback when OnPressed is called.</param>
        /// <param name = "key">Optional key to include within the callback.</param>
        public void Initialize(string text, PressedDelegate callback, string key = "")
        {
            Text = text;
            Key = key;
            _delegate = callback;
        }

        /// <summary>
        /// Called whewn the button for this data is pressed.
        /// </summary>
        public virtual void OnPressed()
        {
            _delegate?.Invoke(Key);
        }

        public virtual void ResetState()
        {
            Text = string.Empty;
            _delegate = null;
            Key = string.Empty;
        }

        public void InitializeState() { }
    }
}