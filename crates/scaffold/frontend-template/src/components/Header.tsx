import React from 'react'
import { WalletConnect } from './WalletConnect'
import NetworkBadge from './NetworkBadge'
import Image from 'next/image'
import logo from '@/public/logo.png'
import github from '@/public/icon.png'

function Header() {
  return (
    <header className="
      mt-[20px] md:mt-[50px] 
      mx-auto 
      px-4 md:px-6 
      py-4 
      bg-[#1F1E1F] 
      border-[#1F1E1F] 
      flex flex-col md:flex-row 
      items-center justify-between 
      w-[95%] max-w-[788px] 
      rounded-[24px] md:rounded-[32px]
      gap-4 md:gap-0
    ">
      <div className="flex items-center gap-1">
        <div className="flex items-center gap-3">
          <Image src={logo} alt="logo" width={32} height={32} />
          <span className="text-[#FFFFFF] text-[20px] md:text-[24.77px] font-medium font-instrument">
            SfdStacks
          </span>
        </div>  
        <NetworkBadge />
      </div>

      <div className='flex items-center space-x-4 md:space-x-6'>
      <a 
            href="https://github.com/scaffold-stack/scaffold-stack" 
            target="_blank" 
            rel="noopener noreferrer"
            className="no-underline" 
          >
        <div className='hidden sm:flex space-x-2 items-center justify-center cursor-pointer hover:opacity-80 transition-opacity'>
          <Image src={github} alt="github" width={20} height={20} />
          <h1 className='text-[14px] md:text-[16px] text-[#8F8D8E] font-instrument'>Github</h1>
        </div>
        </a>
        <WalletConnect />
      </div>
    </header>
  )
}

export default Header