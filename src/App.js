import React from 'react';
import styled, { createGlobalStyle, keyframes } from 'styled-components';

// ===== Animation Definitions =====
const scrollFade = keyframes`
  from { opacity: 0; transform: translateY(20px); }
  to { opacity: 1; transform: translateY(0); }
`;

// ===== Global Styles =====
const GlobalStyle = createGlobalStyle`
  :root {
    --bg: linear-gradient(145deg, #0a0a0a 0%, #121212 100%);
    --card-bg: rgba(25, 25, 25, 0.9);
    --text: #e0e0e0;
    --accent: #2b7a78;
    --hover: #3daaaa;
  }

  * {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }

  body {
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    font-family: 'Inter', system-ui, sans-serif;
    padding: 2rem 1rem;
    min-height: 100vh;
  }

  a {
    color: var(--hover);
    text-decoration: none;
    transition: opacity 0.2s ease;

    &:hover {
      opacity: 0.8;
      text-decoration: underline;
    }
  }

  /* Bullet point fixes */
  ul {
    padding-left: 1.5rem;
    margin: 0.8rem 0;
    
    li {
      margin-bottom: 0.5rem;
      max-width: 80ch;
      line-height: 1.5;
    }
  }

  @media (max-width: 768px) {
    ul {
      padding-left: 1.2rem;
      
      li {
        margin-left: 0.5rem;
      }
    }
  }
`;

// ===== Styled Components =====
const Container = styled.div`
  max-width: 1200px;
  margin: 0 auto;
`;

const Header = styled.header`
  text-align: center;
  margin-bottom: 3rem;
  animation: ${scrollFade} 0.8s ease;

  h1 {
    color: var(--hover);
    font-size: 2.8rem;
    margin-bottom: 1rem;
    letter-spacing: -0.05em;
  }

  p {
    max-width: 600px;
    margin: 0 auto;
    font-size: 1.1rem;
    opacity: 0.9;
  }
`;

const ButtonGroup = styled.div`
  display: flex;
  gap: 1rem;
  justify-content: center;
  margin: 2rem 0;

  @media (max-width: 768px) {
    flex-direction: column;
  }
`;

const Button = styled.a`
  padding: 0.8rem 1.5rem;
  border-radius: 6px;
  text-decoration: none;
  font-weight: 500;
  transition: 0.2s all ease;
  cursor: pointer;

  &.primary {
    background: var(--accent);
    color: white;

    &:hover {
      background: var(--hover);
    }
  }

  &.secondary {
    border: 1px solid var(--accent);
    color: var(--accent);

    &:hover {
      border-color: var(--hover);
      color: var(--hover);
    }
  }
`;

const Section = styled.section`
  background: var(--card-bg);
  border-radius: 8px;
  padding: 2rem;
  margin-bottom: 2rem;

  h2 {
    color: var(--accent);
    margin-bottom: 1.5rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid #333;
  }
`;

const FeatureShowcase = styled.section`
  display: grid;
  gap: 2rem;
  margin: 4rem 0 2rem;
  animation: ${scrollFade} 0.8s ease;

  @media (min-width: 768px) {
    grid-template-columns: 1fr 1fr;
    align-items: start;
  }
`;

const Screenshot = styled.div`
  border-radius: 12px;
  overflow: hidden;
  border: 1px solid #333;
  transition: transform 0.3s ease;
  
  &:hover {
    transform: translateY(-5px);
  }

  img {
    width: 100%;
    height: auto;
    display: block;
  }

  figcaption {
    padding: 1rem;
    background: var(--card-bg);
    font-size: 0.9rem;
    opacity: 0.8;
  }
`;

const FullFeatureList = styled.div`
  columns: 2;
  gap: 2rem;
  margin-top: 2rem;

  @media (max-width: 768px) {
    columns: 1;
  }

  div {
    break-inside: avoid;
    padding: 1rem;
    background: var(--card-bg);
    border-radius: 8px;
    margin-bottom: 1rem;
    
    h3 {
      color: var(--hover);
      margin-bottom: 0.5rem;
    }
    
    ul {
      padding-left: 1.2rem;
    }
  }
`;

const Credits = styled.footer`
  opacity: 0.8;
  font-size: 0.9rem;
  margin-top: 2rem;
  padding-top: 2rem;
  border-top: 1px solid #333;

  a {
    color: var(--hover);
  }
`;

// ===== Updated Main Component =====
const App = () => {
  return (
    <>
      <GlobalStyle />
      <Container>
        <Header>
          <h1>Roblox Macro Utilities</h1>
          <p>Windows automation tool that works alongside Roblox - no memory access, just input automation</p>
          
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download (1.3MB EXE)
            </Button>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities" className="secondary">
              Source Code
            </Button>
          </ButtonGroup>
        </Header>

        <FeatureShowcase>
          <div>
            <h2>Customizable Interface</h2>
            <p>Control how you interact with the tools:</p>
            <ul>
              <li>Drag buttons to any position</li>
              <li>Resize window to your preference</li>
              <li>Dark theme follows system settings</li>
              <li>Layout saves automatically</li>
            </ul>
          </div>
          <Screenshot>
            {/* Keep placeholder */}
          </Screenshot>
        </FeatureShowcase>

        <Section>
          <h2>Complete Feature List</h2>
          <FullFeatureList>
            <div>
              <h3>Essential Features</h3>
              <ul>
                <li>Persistent anti-AFK</li>
                <li>Single executable (no install)</li>
                <li>Update notifications</li>
                <li>Low resource usage</li>
              </ul>
            </div>

            <div>
              <h3>Movement Macros</h3>
              <ul>
                <li>Helicopter High Jump</li>
                <li>Speedglitch toggle</li>
                <li>Automatic Ledge Bouncing</li>
                <li>Wallhop/Wall-Walk</li>
                <li>14-Stud Lag High Jump</li>
              </ul>
            </div>

            <div>
              <h3>Advanced Functions</h3>
              <ul>
                <li>Item Desync Hitboxes</li>
                <li>Microsecond input timing</li>
                <li>Key spam repeater</li>
                <li>Process suspension</li>
                <li>Laugh Clipping</li>
              </ul>
            </div>

            <div>
              <h3>Technical Details</h3>
              <ul>
                <li>External input simulation</li>
                <li>Portable execution</li>
                <li>Open source C++</li>
                <li>Active maintenance</li>
              </ul>
            </div>
          </FullFeatureList>
        </Section>

        <FeatureShowcase>
          <Screenshot>
            {/* Keep placeholder */}
          </Screenshot>
          <div>
            <h2>Precise Control</h2>
            <p>Detailed configuration options:</p>
            <ul>
              <li>Microsecond timing accuracy</li>
              <li>Custom keybind mapping</li>
              <li>Active macro indicators</li>
              <li>Toggle state visibility</li>
            </ul>
          </div>
        </FeatureShowcase>

        <Section style={{ textAlign: 'center', marginTop: '4rem' }}>
          <h2>Get Started</h2>
          <ButtonGroup>
            <Button href="https://github.com/Spencer0187/Roblox-Macro-Utilities/releases/latest" className="primary">
              Download (1.3MB)
            </Button>
            <Button href="https://discord.gg/roblox-glitching-community-998572881892094012" className="secondary">
              Join Community
            </Button>
          </ButtonGroup>
          <p style={{ marginTop: '1rem', opacity: 0.8 }}>
            Windows 10/11 · Portable EXE · No dependencies
          </p>
          <p style={{ marginTop: '1rem', opacity: 0.7, fontSize: '0.9rem' }}>
            Discord features update pings and glitch discussion
          </p>
        </Section>

        <Credits>
          <p>Implementation Details:</p>
          <ul>
            <li><a href="https://github.com/ocornut/imgui">ImGui</a> interface framework</li>
            <li><a href="https://github.com/craftwar/suspend">suspend</a> process handling</li>
          </ul>
        </Credits>
      </Container>
    </>
  );
};

export default App;
