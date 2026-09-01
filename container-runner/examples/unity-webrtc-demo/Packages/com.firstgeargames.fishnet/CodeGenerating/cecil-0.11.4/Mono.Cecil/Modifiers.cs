//
// Author:
//   Jb Evain (jbevain@gmail.com)
//
// Copyright (c) 2008 - 2015 Jb Evain
// Copyright (c) 2008 - 2011 Novell, Inc.
//
// Licensed under the MIT/X11 license.
//

using System;
using MD = MonoFN.Cecil.Metadata;

namespace MonoFN.Cecil
{
    public interface IModifierType
    {
        TypeReference ModifierType { get; }
        TypeReference ElementType { get; }
    }

    public sealed class OptionalModifierType : TypeSpecification, IModifierType
    {
        public TypeReference ModifierType { get; set; }
        public override string Name
        {
            get { return base.Name + Suffix; }
        }
        public override string FullName
        {
            get { return base.FullName + Suffix; }
        }
        private string Suffix
        {
            get { return " modopt(" + ModifierType + ")"; }
        }
        public override bool IsValueType
        {
            get { return false; }
            set { throw new InvalidOperationException(); }
        }
        public override bool IsOptionalModifier
        {
            get { return true; }
        }
        public override bool ContainsGenericParameter
        {
            get { return ModifierType.ContainsGenericParameter || base.ContainsGenericParameter; }
        }

        public OptionalModifierType(TypeReference modifierType, TypeReference type) : base(type)
        {
            if (modifierType == null)
                throw new ArgumentNullException(Mixin.Argument.modifierType.ToString());
            Mixin.CheckType(type);
            ModifierType = modifierType;
            etype = MD.ElementType.CModOpt;
        }
    }

    public sealed class RequiredModifierType : TypeSpecification, IModifierType
    {
        public TypeReference ModifierType { get; set; }
        public override string Name
        {
            get { return base.Name + Suffix; }
        }
        public override string FullName
        {
            get { return base.FullName + Suffix; }
        }
        private string Suffix
        {
            get { return " modreq(" + ModifierType + ")"; }
        }
        public override bool IsValueType
        {
            get { return false; }
            set { throw new InvalidOperationException(); }
        }
        public override bool IsRequiredModifier
        {
            get { return true; }
        }
        public override bool ContainsGenericParameter
        {
            get { return ModifierType.ContainsGenericParameter || base.ContainsGenericParameter; }
        }

        public RequiredModifierType(TypeReference modifierType, TypeReference type) : base(type)
        {
            if (modifierType == null)
                throw new ArgumentNullException(Mixin.Argument.modifierType.ToString());
            Mixin.CheckType(type);
            ModifierType = modifierType;
            etype = MD.ElementType.CModReqD;
        }
    }
}