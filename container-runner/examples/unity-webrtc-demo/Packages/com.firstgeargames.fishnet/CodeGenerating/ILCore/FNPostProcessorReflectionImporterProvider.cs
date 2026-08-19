using MonoFN.Cecil;

namespace FishNet.CodeGenerating.ILCore
{
    internal class FNPostProcessorReflectionImporterProvider : IReflectionImporterProvider
    {
        public IReflectionImporter GetReflectionImporter(ModuleDefinition moduleDef)
        {
            return new FNPostProcessorReflectionImporter(moduleDef);
        }
    }
}